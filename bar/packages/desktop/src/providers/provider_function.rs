use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "function", rename_all = "snake_case")]
pub enum ProviderFunction {
  Audio(AudioFunction),
  // `glazewm` is still accepted so widget packs written against the
  // upstream bar keep loading.
  #[serde(rename = "ninja", alias = "glazewm")]
  Ninja(NinjaFunction),
  Media(MediaFunction),
  Systray(SystrayFunction),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "name", content = "args", rename_all = "snake_case")]
pub enum NinjaFunction {
  RunCommand(RunCommandArgs),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunCommandArgs {
  pub command: String,
  pub subject_container_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "name", content = "args", rename_all = "snake_case")]
pub enum AudioFunction {
  SetVolume(SetVolumeArgs),
  SetMute(SetMuteArgs),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetVolumeArgs {
  pub volume: f32,
  pub device_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetMuteArgs {
  pub mute: bool,
  pub device_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "name", content = "args", rename_all = "snake_case")]
pub enum MediaFunction {
  Play(MediaControlArgs),
  Pause(MediaControlArgs),
  TogglePlayPause(MediaControlArgs),
  Next(MediaControlArgs),
  Previous(MediaControlArgs),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaControlArgs {
  pub session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "name", content = "args", rename_all = "snake_case")]
// The `Icon` prefix is part of the widget-facing `name` field (e.g.
// `icon_hover_enter`); dropping it would break existing widget packs.
#[allow(clippy::enum_variant_names)]
pub enum SystrayFunction {
  IconHoverEnter(SystrayIconArgs),
  IconHoverLeave(SystrayIconArgs),
  IconHoverMove(SystrayIconArgs),
  IconLeftClick(SystrayIconArgs),
  IconLeftDoubleClick(SystrayIconArgs),
  IconRightClick(SystrayIconArgs),
  IconMiddleClick(SystrayIconArgs),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystrayIconArgs {
  pub icon_id: String,
}

pub type ProviderFunctionResult = Result<ProviderFunctionResponse, String>;

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum ProviderFunctionResponse {
  Null,
  NinjaSubjectContainerId(String),
}
