use std::{collections::HashMap, env, fs, path::PathBuf};

use anyhow::{Context, Result};
use tracing::warn;
use wm_common::{
  InvokeCommand, InvokeFocusCommand, InvokeMoveCommand, KeybindingConfig,
  MatchType, ParsedConfig, WindowMatchConfig, WindowRuleConfig,
  WindowRuleEvent, WorkspaceConfig,
};
use wm_platform::{Key, Keybinding};

use crate::{
  models::{Monitor, WindowContainer, Workspace},
  traits::{CommonGetters, WindowGetters},
  workspace_store,
};

/// Resource string for the sample config file.
const SAMPLE_CONFIG: &str =
  include_str!("../../../resources/assets/sample-config.yaml");

#[derive(Debug)]
pub struct UserConfig {
  /// Path to the user config file.
  pub path: PathBuf,

  /// Parsed user config value.
  pub value: ParsedConfig,

  /// Unparsed user config string.
  pub value_str: String,

  /// Hashmap of window rule event types (e.g. `WindowRuleEvent::Manage`)
  /// and the corresponding window rules of that type.
  window_rules_by_event: HashMap<WindowRuleEvent, Vec<WindowRuleConfig>>,
}

impl UserConfig {
  /// Creates an instance of `UserConfig`. Reads and validates the user
  /// config from the given path.
  ///
  /// Creates a new config file from sample if it doesn't exist.
  pub fn new(config_path: Option<PathBuf>) -> anyhow::Result<Self> {
    let default_config_path = wm_common::ninja_dir()?.join("config.yaml");

    // `GLAZEWM_CONFIG_PATH` is still read so setups carried over from
    // before the rename keep working.
    let config_path = config_path
      .or_else(|| env::var("NINJA_CONFIG_PATH").ok().map(PathBuf::from))
      .or_else(|| env::var("GLAZEWM_CONFIG_PATH").ok().map(PathBuf::from))
      .unwrap_or(default_config_path);

    let (config_value, config_str) = Self::read(&config_path)?;

    let window_rules_by_event = Self::window_rules_by_event(&config_value);

    Ok(Self {
      path: config_path,
      value: config_value,
      value_str: config_str,
      window_rules_by_event,
    })
  }

  /// Reads and validates the user config from the given path.
  ///
  /// Creates a new config file from sample if it doesn't exist.
  fn read(
    config_path: &PathBuf,
  ) -> anyhow::Result<(ParsedConfig, String)> {
    if !config_path.exists() {
      Self::create_sample(config_path)?;
    }

    let config_str = fs::read_to_string(config_path)
      .context("Unable to read config file.")?;

    // TODO: Improve error formatting of serde_yaml errors. Something
    // similar to https://github.com/AlexanderThaller/format_serde_error
    let mut config_value: ParsedConfig =
      serde_yaml::from_str(&config_str)?;

    // Workspaces added at runtime live in their own file, so that the WM
    // never rewrites the user's config. The config takes precedence over
    // the store for any name declared in both.
    workspace_store::merge_into(
      &mut config_value.workspaces,
      &workspace_store::read(config_path),
    );

    Self::add_extended_workspace_keybindings(&mut config_value);

    Ok((config_value, config_str))
  }

  /// Initializes a new config file from the sample config resource.
  fn create_sample(config_path: &PathBuf) -> Result<()> {
    let parent_dir =
      config_path.parent().context("Invalid config path.")?;

    fs::create_dir_all(parent_dir).with_context(|| {
      format!("Unable to create directory {}.", config_path.display())
    })?;

    fs::write(config_path, SAMPLE_CONFIG).with_context(|| {
      format!("Unable to write to {}.", config_path.display())
    })?;

    Ok(())
  }

  pub fn reload(&mut self) -> anyhow::Result<()> {
    let (config_value, config_str) = Self::read(&self.path)?;

    self.window_rules_by_event =
      Self::window_rules_by_event(&config_value);
    self.value = config_value;
    self.value_str = config_str;

    Ok(())
  }

  fn default_window_rules(
    config_value: &ParsedConfig,
  ) -> Vec<WindowRuleConfig> {
    let mut window_rules = Vec::new();

    let floating_defaults =
      &config_value.window_behavior.state_defaults.floating;

    // Default float rules.
    window_rules.push(WindowRuleConfig {
      commands: vec![InvokeCommand::SetFloating {
        centered: Some(floating_defaults.centered),
        shown_on_top: Some(floating_defaults.shown_on_top),
        x_pos: None,
        y_pos: None,
        width: None,
        height: None,
      }],
      match_window: vec![
        WindowMatchConfig {
          window_class: Some(MatchType::Equals { equals:
          // W10/W11 system dialog shown when moving and deleting files.
          "OperationStatusWindow".to_string(),
        }),
          ..WindowMatchConfig::default()
        },
        WindowMatchConfig {
          window_class: Some(MatchType::Equals { equals:
          // W10/W11 system dialogs (e.g. File Explorer save/open dialog).
          "#32770".to_string(),
        }),
          ..WindowMatchConfig::default()
        },
      ],
      on: vec![WindowRuleEvent::Manage],
      run_once: true,
    });

    // Default ignore rules.
    window_rules.push(WindowRuleConfig {
      commands: vec![InvokeCommand::Ignore],
      match_window: vec![
        WindowMatchConfig {
          window_process: Some(MatchType::Equals {
            equals: "SearchApp".to_string(),
          }),
          ..WindowMatchConfig::default()
        },
        WindowMatchConfig {
          window_process: Some(MatchType::Equals {
            equals: "SearchHost".to_string(),
          }),
          ..WindowMatchConfig::default()
        },
        WindowMatchConfig {
          window_process: Some(MatchType::Equals {
            equals: "ShellExperienceHost".to_string(),
          }),
          ..WindowMatchConfig::default()
        },
        WindowMatchConfig {
          window_process: Some(MatchType::Equals {
            // W10/11 start menu.
            equals: "StartMenuExperienceHost".to_string(),
          }),
          ..WindowMatchConfig::default()
        },
        WindowMatchConfig {
          window_process: Some(MatchType::Equals {
            // W10/11 screen snipping tool.
            equals: "ScreenClippingHost".to_string(),
          }),
          ..WindowMatchConfig::default()
        },
        WindowMatchConfig {
          window_process: Some(MatchType::Equals {
            // W11 lock screen.
            equals: "LockApp".to_string(),
          }),
          ..WindowMatchConfig::default()
        },
      ],
      on: vec![WindowRuleEvent::Manage],
      run_once: true,
    });

    window_rules
  }

  fn window_rules_by_event(
    config_value: &ParsedConfig,
  ) -> HashMap<WindowRuleEvent, Vec<WindowRuleConfig>> {
    let mut window_rules_by_event = HashMap::new();

    // Combine user-defined window rules with the default ones.
    let default_window_rules = Self::default_window_rules(config_value);
    let all_window_rules = config_value
      .window_rules
      .iter()
      .chain(default_window_rules.iter());

    for window_rule in all_window_rules {
      for event_type in &window_rule.on {
        window_rules_by_event
          .entry(event_type.clone())
          .or_insert_with(Vec::new)
          .push(window_rule.clone());
      }
    }

    window_rules_by_event
  }

  /// Window rules that should be applied to the window when the given
  /// event occurs.
  pub fn pending_window_rules(
    &self,
    window: &WindowContainer,
    event: &WindowRuleEvent,
  ) -> Vec<WindowRuleConfig> {
    let window_title = window.native_properties().title;
    #[cfg(target_os = "windows")]
    let window_class = window.native_properties().class_name;
    let window_process = window.native_properties().process_name;

    let pending_window_rules = self
      .window_rules_by_event
      .get(event)
      .unwrap_or(&Vec::new())
      .iter()
      .filter(|rule| {
        // Skip if window has already ran the rule.
        if window.done_window_rules().contains(rule) {
          return false;
        }

        // Check if the window matches the rule.
        rule.match_window.iter().any(|match_config| {
          let is_process_match = match_config
            .window_process
            .as_ref()
            .is_none_or(|match_type| {
              // TODO: Temp fix for matching the bar on both platforms with
              // the same process name. Consider using lowercase for every
              // `equals` match type.
              if window_process == "Ninja" {
                match_type.is_match("Ninja")
                  || match_type.is_match("ninja")
              } else {
                match_type.is_match(&window_process)
              }
            });

          let is_class_match = {
            #[cfg(target_os = "windows")]
            {
              match_config.window_class.as_ref().is_none_or(|match_type| {
                match_type.is_match(&window_class)
              })
            }
            #[cfg(not(target_os = "windows"))]
            {
              match_config.window_class.is_none()
            }
          };

          let is_title_match = match_config
            .window_title
            .as_ref()
            .is_none_or(|match_type| match_type.is_match(&window_title));

          is_process_match && is_class_match && is_title_match
        })
      })
      .cloned()
      .collect::<Vec<_>>();

    pending_window_rules
  }

  pub fn inactive_workspace_configs(
    &self,
    active_workspaces: &[Workspace],
  ) -> Vec<&WorkspaceConfig> {
    self
      .value
      .workspaces
      .iter()
      .filter(|config| {
        !active_workspaces
          .iter()
          .any(|workspace| workspace.config().name == config.name)
      })
      .collect()
  }

  pub fn workspace_config_for_monitor(
    &self,
    monitor: &Monitor,
    active_workspaces: &[Workspace],
  ) -> Option<&WorkspaceConfig> {
    let inactive_configs =
      self.inactive_workspace_configs(active_workspaces);

    let monitor_id = monitor.stable_id();
    let monitor_index = monitor.index();

    inactive_configs.into_iter().find(|&config| {
      config.matches_monitor(monitor_id.as_deref(), monitor_index)
    })
  }

  /// Gets the first inactive workspace config, prioritizing configs that
  /// don't have a monitor binding.
  pub fn next_inactive_workspace_config(
    &self,
    active_workspaces: &[Workspace],
  ) -> Option<&WorkspaceConfig> {
    let inactive_configs =
      self.inactive_workspace_configs(active_workspaces);

    inactive_configs
      .iter()
      .find(|config| !config.is_bound())
      .or(inactive_configs.first())
      .copied()
  }

  pub fn workspace_config_index(
    &self,
    workspace_name: &str,
  ) -> Option<usize> {
    self
      .value
      .workspaces
      .iter()
      .position(|config| config.name == workspace_name)
  }

  /// Mirrors the first ten workspace keybindings onto the next ten.
  ///
  /// A binding for workspace `n` (1-10, where 10 is usually bound to `0`)
  /// gains a counterpart with `second_modifier` held that targets
  /// `n + 10`. Both `focus` and `move` bindings are mirrored, so moving a
  /// window to workspace 11 works the same way as focusing it.
  ///
  /// Generated bindings are skipped when the user has already bound that
  /// combination, so hand-written config always wins.
  fn add_extended_workspace_keybindings(config: &mut ParsedConfig) {
    let extended = &config.extended_workspaces;

    if !extended.enabled {
      return;
    }

    let Ok(modifier) = extended.second_modifier.parse::<Key>() else {
      warn!(
        "Invalid `extended_workspaces.second_modifier`: '{}'.          Skipping generated workspace keybindings.",
        extended.second_modifier
      );

      return;
    };

    let mut generated = Vec::new();

    for keybinding in &config.keybindings {
      for command in &keybinding.commands {
        let Some(target) = Self::extended_workspace_command(command)
        else {
          continue;
        };

        for binding in &keybinding.bindings {
          // Leave alone anything the user already bound with this
          // modifier, and anything that already uses it.
          if binding.keys().contains(&modifier) {
            continue;
          }

          let mut keys = vec![modifier];
          keys.extend_from_slice(binding.keys());

          let Ok(extended_binding) = Keybinding::new(keys) else {
            continue;
          };

          if Self::is_binding_taken(&config.keybindings, &extended_binding)
          {
            continue;
          }

          generated.push(KeybindingConfig {
            bindings: vec![extended_binding],
            commands: vec![target.clone()],
          });
        }
      }
    }

    config.keybindings.extend(generated);
  }

  /// Maps a workspace command for 1-10 to the same command for 11-20.
  fn extended_workspace_command(
    command: &InvokeCommand,
  ) -> Option<InvokeCommand> {
    let shift = |name: &Option<String>| -> Option<String> {
      let number = name.as_ref()?.parse::<u32>().ok()?;
      (1..=10)
        .contains(&number)
        .then(|| (number + 10).to_string())
    };

    match command {
      InvokeCommand::Focus(args) => {
        let workspace = shift(&args.workspace)?;

        Some(InvokeCommand::Focus(InvokeFocusCommand {
          workspace: Some(workspace),
          ..args.clone()
        }))
      }
      InvokeCommand::Move(args) => {
        let workspace = shift(&args.workspace)?;

        Some(InvokeCommand::Move(InvokeMoveCommand {
          workspace: Some(workspace),
          ..args.clone()
        }))
      }
      _ => None,
    }
  }

  /// Whether any existing keybinding already uses this key combination.
  fn is_binding_taken(
    keybindings: &[KeybindingConfig],
    binding: &Keybinding,
  ) -> bool {
    keybindings.iter().any(|keybinding| {
      keybinding
        .bindings
        .iter()
        .any(|existing| existing.keys() == binding.keys())
    })
  }

  pub fn sort_workspaces(&self, workspaces: &mut [Workspace]) {
    workspaces.sort_by_key(|workspace| {
      Self::workspace_sort_key(
        &workspace.config().name,
        self.workspace_config_index(&workspace.config().name),
      )
    });
  }

  /// Ordering key for a workspace within its monitor.
  ///
  /// Declared workspaces keep the order they appear in the config.
  /// Dynamically created ones sort after those, numerically where the
  /// name is a number and alphabetically otherwise.
  ///
  /// Without the first tier, an undeclared workspace would sort ahead of
  /// every declared one, so workspace 11 would appear left of workspace 1.
  fn workspace_sort_key(
    name: &str,
    config_index: Option<usize>,
  ) -> (u8, usize, String) {
    match config_index {
      Some(index) => (0, index, String::new()),
      None => match name.parse::<usize>() {
        Ok(number) => (1, number, String::new()),
        Err(_) => (2, 0, name.to_string()),
      },
    }
  }

  /// Keybinding configs that should be active for the current binding mode
  /// and pause state.
  ///
  /// When paused, only the configs with `InvokeCommand::WmTogglePause` are
  /// returned so that unpausing remains possible.
  pub fn active_keybinding_configs(
    &self,
    binding_modes: &[wm_common::BindingModeConfig],
    is_paused: bool,
  ) -> impl Iterator<Item = KeybindingConfig> {
    let source_configs = if let Some(first_mode) = binding_modes.first() {
      &first_mode.keybindings
    } else {
      &self.value.keybindings
    }
    .clone();

    source_configs.into_iter().filter(move |kb| {
      if is_paused {
        kb.commands
          .contains(&wm_common::InvokeCommand::WmTogglePause)
      } else {
        true
      }
    })
  }
}

#[cfg(test)]
impl UserConfig {
  /// Creates a `UserConfig` for use in tests, without touching disk.
  pub fn mock(workspaces: Vec<WorkspaceConfig>) -> Self {
    let value = wm_common::ParsedConfig {
      workspaces,
      ..wm_common::ParsedConfig::default()
    };

    Self {
      path: PathBuf::from("config.yaml"),
      window_rules_by_event: Self::window_rules_by_event(&value),
      value,
      value_str: String::new(),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::UserConfig;

  /// Sorts names the way `sort_workspaces` would, given which of them the
  /// config declares (in the order listed).
  fn sorted(declared: &[&str], mut names: Vec<&str>) -> Vec<&'static str> {
    names.sort_by_key(|name| {
      UserConfig::workspace_sort_key(
        name,
        declared.iter().position(|entry| entry == name),
      )
    });

    names
      .into_iter()
      .map(|name| Box::leak(name.to_string().into_boxed_str()) as &str)
      .collect()
  }

  #[test]
  fn declared_workspaces_keep_config_order() {
    // Config order wins even when it isn't numeric.
    assert_eq!(
      sorted(&["3", "1", "2"], vec!["1", "2", "3"]),
      ["3", "1", "2"]
    );
  }

  #[test]
  fn dynamic_workspaces_sort_after_declared() {
    // The bug this guards: without tiering, "11" sorts before "1".
    assert_eq!(
      sorted(&["1", "2", "3"], vec!["11", "2", "1", "3"]),
      ["1", "2", "3", "11"]
    );
  }

  #[test]
  fn dynamic_workspaces_sort_numerically_not_alphabetically() {
    assert_eq!(sorted(&[], vec!["10", "9", "2"]), ["2", "9", "10"]);
  }

  #[test]
  fn named_dynamic_workspaces_sort_last() {
    assert_eq!(
      sorted(&["1"], vec!["scratch", "10", "1"]),
      ["1", "10", "scratch"]
    );
  }
}

#[cfg(test)]
mod extended_workspace_tests {
  use wm_common::{
    InvokeCommand, InvokeFocusCommand, KeybindingConfig, ParsedConfig,
  };
  use wm_platform::{Key, Keybinding};

  use super::UserConfig;

  fn focus_workspace(name: &str) -> InvokeCommand {
    InvokeCommand::Focus(InvokeFocusCommand {
      workspace: Some(name.to_string()),
      direction: None,
      container_id: None,
      workspace_in_direction: None,
      monitor: None,
      next_active_workspace: false,
      prev_active_workspace: false,
      next_workspace: false,
      prev_workspace: false,
      next_active_workspace_on_monitor: false,
      prev_active_workspace_on_monitor: false,
      recent_workspace: false,
    })
  }

  fn config_with(bindings: Vec<KeybindingConfig>) -> ParsedConfig {
    ParsedConfig {
      keybindings: bindings,
      ..ParsedConfig::default()
    }
  }

  /// Finds the command bound to a given key combination.
  fn command_for(config: &ParsedConfig, keys: &[Key]) -> Option<String> {
    config.keybindings.iter().find_map(|keybinding| {
      keybinding
        .bindings
        .iter()
        .any(|binding| binding.keys() == keys)
        .then(|| match &keybinding.commands[0] {
          InvokeCommand::Focus(args) => args.workspace.clone(),
          _ => None,
        })
        .flatten()
    })
  }

  #[test]
  fn mirrors_workspaces_one_to_ten() {
    let mut config = config_with(vec![
      KeybindingConfig {
        bindings: vec![Keybinding::new(vec![Key::Alt, Key::D1]).unwrap()],
        commands: vec![focus_workspace("1")],
      },
      KeybindingConfig {
        bindings: vec![Keybinding::new(vec![Key::Alt, Key::D0]).unwrap()],
        commands: vec![focus_workspace("10")],
      },
    ]);

    UserConfig::add_extended_workspace_keybindings(&mut config);

    assert_eq!(
      command_for(&config, &[Key::Ctrl, Key::Alt, Key::D1]),
      Some("11".to_string())
    );
    assert_eq!(
      command_for(&config, &[Key::Ctrl, Key::Alt, Key::D0]),
      Some("20".to_string())
    );
  }

  #[test]
  fn leaves_originals_untouched() {
    let mut config = config_with(vec![KeybindingConfig {
      bindings: vec![Keybinding::new(vec![Key::Alt, Key::D1]).unwrap()],
      commands: vec![focus_workspace("1")],
    }]);

    UserConfig::add_extended_workspace_keybindings(&mut config);

    assert_eq!(
      command_for(&config, &[Key::Alt, Key::D1]),
      Some("1".to_string())
    );
  }

  #[test]
  fn does_not_override_a_user_binding() {
    // The user already bound ctrl+alt+1 to something else.
    let mut config = config_with(vec![
      KeybindingConfig {
        bindings: vec![Keybinding::new(vec![Key::Alt, Key::D1]).unwrap()],
        commands: vec![focus_workspace("1")],
      },
      KeybindingConfig {
        bindings: vec![Keybinding::new(vec![
          Key::Ctrl,
          Key::Alt,
          Key::D1,
        ])
        .unwrap()],
        commands: vec![focus_workspace("7")],
      },
    ]);

    UserConfig::add_extended_workspace_keybindings(&mut config);

    assert_eq!(
      command_for(&config, &[Key::Ctrl, Key::Alt, Key::D1]),
      Some("7".to_string())
    );
  }

  #[test]
  fn ignores_workspaces_past_ten() {
    // Workspace 11 must not generate a binding for workspace 21.
    let mut config = config_with(vec![KeybindingConfig {
      bindings: vec![Keybinding::new(vec![Key::Win, Key::D1]).unwrap()],
      commands: vec![focus_workspace("11")],
    }]);

    UserConfig::add_extended_workspace_keybindings(&mut config);

    assert_eq!(config.keybindings.len(), 1);
  }

  #[test]
  fn disabled_generates_nothing() {
    let mut config = config_with(vec![KeybindingConfig {
      bindings: vec![Keybinding::new(vec![Key::Alt, Key::D1]).unwrap()],
      commands: vec![focus_workspace("1")],
    }]);

    config.extended_workspaces.enabled = false;
    UserConfig::add_extended_workspace_keybindings(&mut config);

    assert_eq!(config.keybindings.len(), 1);
  }
}
