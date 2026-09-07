use std::{iter, time::Duration};

use async_trait::async_trait;
use clap::Parser;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::{sync::mpsc, task};
use tracing::{debug, info, warn};
use wm::LocalIpcClient;
use wm_common::{AppCommand, ClientResponseData, InvokeCommand, ServerMessage};

use crate::providers::{
  CommonProviderState, NinjaFunction, Provider, ProviderFunction,
  ProviderFunctionResponse, ProviderInputMsg, RuntimeType,
};

/// Capacity of the channel carrying WM events into the provider loop.
const EVENT_BUFFER_SIZE: usize = 64;

/// Delay before the first retry after a failed connection.
const RECONNECT_DELAY_MIN: Duration = Duration::from_secs(1);

/// Longest delay between connection attempts.
///
/// The delay doubles from `RECONNECT_DELAY_MIN` up to this ceiling, so a
/// WM that stays down doesn't get hammered, but one that comes back is
/// picked up within a few seconds.
const RECONNECT_DELAY_MAX: Duration = Duration::from_secs(10);

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct NinjaProviderConfig {}

/// Raw window manager state, forwarded to the frontend as-is.
///
/// The values are kept as JSON rather than typed DTOs because the
/// frontend derives per-widget state from them (e.g. which monitor a
/// given widget sits on), and because `ProviderOutput` requires
/// `PartialEq`, which the WM's DTOs don't implement.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NinjaOutput {
  pub all_monitors: Value,
  pub all_windows: Value,
  pub focused_container: Value,
  pub tiling_direction: Value,
  pub binding_modes: Value,
  pub is_paused: bool,
}

pub struct NinjaProvider {
  common: CommonProviderState,
}

impl NinjaProvider {
  pub fn new(
    _config: NinjaProviderConfig,
    common: CommonProviderState,
  ) -> NinjaProvider {
    NinjaProvider { common }
  }

  /// Sends a message and returns the data from its response.
  async fn query(
    client: &mut LocalIpcClient,
    message: &str,
  ) -> anyhow::Result<ClientResponseData> {
    client.send(message).await?;

    let response = client
      .client_response(message)
      .await
      .ok_or_else(|| anyhow::anyhow!("No response to '{message}'."))?;

    if !response.success {
      anyhow::bail!(
        "Command '{message}' failed: {}",
        response
          .error
          .unwrap_or_else(|| "Unknown error.".to_string())
      );
    }

    response.data.ok_or_else(|| {
      anyhow::anyhow!("No data in response to '{message}'.")
    })
  }

  /// Queries the full WM state.
  ///
  /// Every event triggers a full refresh. The queries are cheap local IPC
  /// round-trips, and this keeps the state unconditionally correct rather
  /// than patching it per event type.
  async fn query_state(
    client: &mut LocalIpcClient,
  ) -> anyhow::Result<NinjaOutput> {
    let all_monitors = match Self::query(client, "query monitors").await? {
      ClientResponseData::Monitors(data) => {
        serde_json::to_value(data.monitors)?
      }
      _ => anyhow::bail!("Unexpected response to 'query monitors'."),
    };

    let all_windows = match Self::query(client, "query windows").await? {
      ClientResponseData::Windows(data) => {
        serde_json::to_value(data.windows)?
      }
      _ => anyhow::bail!("Unexpected response to 'query windows'."),
    };

    let focused_container =
      match Self::query(client, "query focused").await? {
        ClientResponseData::Focused(data) => {
          serde_json::to_value(data.focused)?
        }
        _ => anyhow::bail!("Unexpected response to 'query focused'."),
      };

    let binding_modes =
      match Self::query(client, "query binding-modes").await? {
        ClientResponseData::BindingModes(data) => {
          serde_json::to_value(data.binding_modes)?
        }
        _ => anyhow::bail!("Unexpected response to binding-modes."),
      };

    // The remaining queries are optional, so that a version mismatch
    // degrades rather than taking down the whole provider.
    let tiling_direction =
      match Self::query(client, "query tiling-direction").await {
        Ok(ClientResponseData::TilingDirection(data)) => {
          serde_json::to_value(data.tiling_direction)?
        }
        _ => Value::Null,
      };

    let is_paused = match Self::query(client, "query paused").await {
      Ok(ClientResponseData::Paused(paused)) => paused,
      _ => false,
    };

    Ok(NinjaOutput {
      all_monitors,
      all_windows,
      focused_container,
      tiling_direction,
      binding_modes,
      is_paused,
    })
  }

  /// Connects both IPC clients, retrying until it succeeds.
  ///
  /// Returns `None` if a stop signal arrived while waiting.
  ///
  /// A failed connection is expected rather than fatal: the bar and the WM
  /// start together, and the bar routinely wins the race to the WM's IPC
  /// listener by a few hundred milliseconds.
  async fn connect_with_retry(
    &mut self,
  ) -> Option<(LocalIpcClient, LocalIpcClient)> {
    let mut delay = RECONNECT_DELAY_MIN;

    loop {
      // Two connections: one dedicated to the event subscription, one for
      // queries. Sharing a single connection would let a query consume
      // event messages while it scanned for its own reply.
      if let (Ok(query), Ok(event)) =
        (LocalIpcClient::connect(), LocalIpcClient::connect())
      {
        return Some((query, event));
      }

      debug!("The WM isn't running yet; retrying in {delay:?}.");

      tokio::select! {
        () = tokio::time::sleep(delay) => {}
        Some(input) = self.common.input.async_rx.recv() => {
          match input {
            ProviderInputMsg::Stop => return None,
            // Answered rather than dropped, so the frontend gets a real
            // error instead of a closed channel.
            ProviderInputMsg::Function(_, sender) => {
              let _ = sender
                .send(Err("Not connected to the WM.".to_string()));
            }
          }
        }
      }

      delay = (delay * 2).min(RECONNECT_DELAY_MAX);
    }
  }

  /// Runs a single connected session until the connection drops or the
  /// provider is stopped.
  ///
  /// Returns `true` if the provider should stop entirely, `false` if the
  /// connection ended and should be re-established.
  ///
  /// Errors are logged rather than emitted. The provider reconnects on its
  /// own, so an error is always transient, and emitting one would blank
  /// the workspace indicators in every bar window until the next
  /// refresh.
  async fn run_session(
    &mut self,
    mut query_client: LocalIpcClient,
    mut event_client: LocalIpcClient,
  ) -> bool {
    if let Err(err) = event_client.send("sub -e all").await {
      warn!("Failed to subscribe to WM events: {err:?}");
      return false;
    }

    // Events are pumped into a channel rather than read inside the
    // `select!` below, because the read is not cancel-safe: dropping it
    // midway would desynchronise the connection.
    let (event_tx, mut event_rx) = mpsc::channel(EVENT_BUFFER_SIZE);

    let pump_task = task::spawn(async move {
      while let Ok(message) = event_client.next_message().await {
        if event_tx.send(message).await.is_err() {
          break;
        }
      }
    });

    info!("GlazeWM provider connected; fetching initial state.");

    let should_stop = match Self::query_state(&mut query_client).await {
      Ok(state) => {
        info!(
          "GlazeWM initial state: {} monitors, {} windows.",
          state.all_monitors.as_array().map_or(0, Vec::len),
          state.all_windows.as_array().map_or(0, Vec::len),
        );

        self.common.emitter.emit_output(Ok(state));
        self.run_event_loop(&mut query_client, &mut event_rx).await
      }
      Err(err) => {
        warn!("GlazeWM initial state failed: {err:?}");
        false
      }
    };

    // The pump owns the event connection, so aborting it is what closes
    // that connection. Awaited so the connection is definitely gone before
    // the next one is opened, rather than accumulating against the WM's
    // connection limit across repeated reconnects.
    pump_task.abort();
    let _ = pump_task.await;

    should_stop
  }

  /// Services WM events and frontend function calls for a live connection.
  ///
  /// Returns `true` if the provider should stop entirely, `false` if the
  /// connection ended.
  async fn run_event_loop(
    &mut self,
    query_client: &mut LocalIpcClient,
    event_rx: &mut mpsc::Receiver<ServerMessage>,
  ) -> bool {
    loop {
      tokio::select! {
        message = event_rx.recv() => {
          // `None` means the pump ended, i.e. the event connection is
          // gone. Matched explicitly rather than with `Some(..) =`, which
          // would merely disable this branch and park the loop forever.
          let Some(message) = message else {
            warn!("WM event stream ended; reconnecting.");
            break false;
          };

          // Only event messages warrant a refresh. The reply to the `sub`
          // command itself arrives on this connection too.
          let mut should_refresh =
            matches!(message, ServerMessage::EventSubscription(_));

          // Drain whatever is already queued, so a burst of events costs
          // one refresh rather than one full re-query per event. Each
          // refresh is six sequential IPC round-trips, which is slow
          // enough to back up the WM's outbound queue during a storm (a
          // display wake, for instance).
          while let Ok(queued) = event_rx.try_recv() {
            should_refresh |=
              matches!(queued, ServerMessage::EventSubscription(_));
          }

          if should_refresh {
            debug!("Refreshing WM state after event.");

            match Self::query_state(query_client).await {
              Ok(state) => self.common.emitter.emit_output(Ok(state)),
              Err(err) => {
                warn!("Failed to refresh WM state: {err:?}");
                break false;
              }
            }
          }
        }
        Some(input) = self.common.input.async_rx.recv() => {
          match input {
            ProviderInputMsg::Stop => break true,
            ProviderInputMsg::Function(
              ProviderFunction::Ninja(NinjaFunction::RunCommand(args)),
              sender,
            ) => {
              let res = Self::run_command(
                query_client,
                &args.command,
                args.subject_container_id.as_deref(),
              )
              .await
              .map_err(|err| err.to_string());

              let _ = sender.send(res);
            }
            ProviderInputMsg::Function(..) => {}
          }
        }
      }
    }
  }

  /// Runs a WM command on behalf of the frontend.
  async fn run_command(
    client: &mut LocalIpcClient,
    command: &str,
    subject_container_id: Option<&str>,
  ) -> anyhow::Result<ProviderFunctionResponse> {
    ensure_widget_may_run(command)?;

    // `--id` is an option on the parent `command`, so it has to precede
    // the subcommand rather than follow it.
    let message = match subject_container_id {
      Some(id) => format!("command --id {id} {command}"),
      None => format!("command {command}"),
    };

    match Self::query(client, &message).await? {
      ClientResponseData::Command(data) => {
        Ok(ProviderFunctionResponse::NinjaSubjectContainerId(
          data.subject_container_id.to_string(),
        ))
      }
      _ => anyhow::bail!("Unexpected response to '{message}'."),
    }
  }
}

/// Refuses WM commands that a widget must not run through `runCommand`.
///
/// `shell-exec` is refused: it would let any widget run any program and
/// skip the bar's per-widget shell privileges. Widgets have `shellExec`
/// with a `shellCommands` privilege for that.
fn ensure_widget_may_run(command: &str) -> anyhow::Result<()> {
  let parsed = AppCommand::try_parse_from(
    iter::once("").chain(iter::once("command")).chain(command.split_whitespace()),
  );

  if let Ok(AppCommand::Command {
    command: InvokeCommand::ShellExec { .. },
    ..
  }) = parsed
  {
    anyhow::bail!(
      "'shell-exec' can't be run via runCommand. Use `shellExec` with a \
       `shellCommands` privilege instead."
    );
  }

  Ok(())
}

#[async_trait]
impl Provider for NinjaProvider {
  fn runtime_type(&self) -> RuntimeType {
    RuntimeType::Async
  }

  async fn start_async(&mut self) {
    info!("GlazeWM provider starting; connecting to the WM.");

    // Reconnects for the life of the provider. The connection drops
    // whenever the WM restarts, and previously that left the workspace
    // indicators dead until the bar itself was restarted.
    loop {
      let Some((query_client, event_client)) =
        self.connect_with_retry().await
      else {
        break;
      };

      if self.run_session(query_client, event_client).await {
        break;
      }

      // A session that ends immediately — the WM accepting connections but
      // failing every query, say — would otherwise reconnect in a tight
      // loop. A real WM restart takes far longer than this pause.
      tokio::time::sleep(RECONNECT_DELAY_MIN).await;
    }
  }
}

#[cfg(test)]
mod tests {
  use super::ensure_widget_may_run;

  #[test]
  fn refuses_shell_exec() {
    assert!(ensure_widget_may_run("shell-exec calc").is_err());
  }

  #[test]
  fn refuses_shell_exec_with_flags() {
    assert!(
      ensure_widget_may_run("shell-exec --hide-window cmd /C dir").is_err()
    );
  }

  #[test]
  fn allows_other_commands() {
    assert!(ensure_widget_may_run("focus --workspace 1").is_ok());
    assert!(ensure_widget_may_run("toggle-floating").is_ok());
  }

  #[test]
  fn passes_unparseable_through() {
    assert!(ensure_widget_may_run("not-a-command").is_ok());
  }
}
