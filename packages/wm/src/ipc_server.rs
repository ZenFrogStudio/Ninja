use std::{
  iter,
  sync::{Arc, Mutex},
};

use anyhow::{bail, Context};
use clap::Parser;
use tokio::{
  sync::{broadcast, mpsc, mpsc::error::TrySendError, Semaphore},
  task,
};
use tracing::{debug, info, warn};
use uuid::Uuid;
use wm_common::{
  AppCommand, AppMetadataData, BindingModesData, ClientResponseData,
  ClientResponseMessage, CommandData, ConfigPathData, EventSubscribeData,
  EventSubscriptionMessage, FocusedData, MonitorsData, QueryCommand,
  ServerMessage, SubscribableEvent, TilingDirectionData, WindowsData,
  WmEvent, WorkspacesData,
};
use wm_platform::{
  buf_reader, read_message, write_message, IpcListener, IpcStream,
};

use crate::{
  traits::{CommonGetters, TilingDirectionGetters},
  user_config::UserConfig,
  wm::WindowManager,
};

/// Maximum number of concurrent IPC connections.
const MAX_CONNECTIONS: usize = 32;

/// Capacity of the channel carrying client messages to the main loop.
///
/// Bounded so that a client sending faster than the WM can process is
/// slowed down by backpressure instead of growing this queue without
/// limit.
const MESSAGE_BUFFER_SIZE: usize = 256;

/// Capacity of the per-connection channel carrying responses and events
/// back to a client.
///
/// A client that doesn't read fast enough is disconnected rather than
/// buffered indefinitely.
const RESPONSE_BUFFER_SIZE: usize = 256;

/// Message channel carrying a client message, the connection's response
/// sender, and the connection's disconnection broadcaster.
type MessageChannel =
  (String, mpsc::Sender<String>, broadcast::Sender<()>);

/// Inbound channel of the running IPC server.
///
/// Lets code in this process reach the server without going out to the
/// socket and back. Exactly one WM runs per process — `SingleInstance`
/// enforces it — so a single slot is enough.
///
/// Holds `None` while no server is running, which is what
/// [`LocalIpcClient::connect`] reports as a failure to connect.
static LOCAL_MESSAGE_TX: Mutex<Option<mpsc::Sender<MessageChannel>>> =
  Mutex::new(None);

/// Client that talks to the IPC server from within the WM's own process.
///
/// Speaks the same command strings as [`wm_ipc_client::IpcClient`] and is
/// served by the same handlers; it just skips the socket. The surface is
/// deliberately identical, so callers can be written against either.
pub struct LocalIpcClient {
  message_tx: mpsc::Sender<MessageChannel>,
  response_tx: mpsc::Sender<String>,
  response_rx: mpsc::Receiver<String>,
  disconnection_tx: broadcast::Sender<()>,
}

impl LocalIpcClient {
  /// Connects to the IPC server running in this process.
  ///
  /// # Errors
  ///
  /// Returns an error if no WM is running yet. Callers are expected to
  /// retry, since the WM and its in-process clients start concurrently.
  pub fn connect() -> anyhow::Result<Self> {
    let message_tx = LOCAL_MESSAGE_TX
      .lock()
      .map_err(|_| anyhow::anyhow!("IPC server lock was poisoned."))?
      .clone()
      .context("No IPC server is running in this process.")?;

    let (response_tx, response_rx) =
      mpsc::channel::<String>(RESPONSE_BUFFER_SIZE);
    let (disconnection_tx, _) = broadcast::channel(16);

    Ok(Self {
      message_tx,
      response_tx,
      response_rx,
      disconnection_tx,
    })
  }

  /// Sends a message to the IPC server.
  ///
  /// # Errors
  ///
  /// Returns an error if the server has stopped.
  pub async fn send(&mut self, message: &str) -> anyhow::Result<()> {
    self
      .message_tx
      .send((
        message.to_string(),
        self.response_tx.clone(),
        self.disconnection_tx.clone(),
      ))
      .await
      .context("IPC server is no longer accepting messages.")?;

    Ok(())
  }

  /// Waits and returns the next reply from the IPC server.
  ///
  /// # Errors
  ///
  /// Returns an error if the server has stopped, or if a reply can't be
  /// parsed.
  pub async fn next_message(&mut self) -> anyhow::Result<ServerMessage> {
    let response = self
      .response_rx
      .recv()
      .await
      .context("IPC connection closed.")?;

    Ok(serde_json::from_str::<ServerMessage>(&response)?)
  }

  /// Waits for the response to a specific message, discarding any events
  /// that arrive in the meantime.
  pub async fn client_response(
    &mut self,
    client_message: &str,
  ) -> Option<ClientResponseMessage> {
    while let Ok(response) = self.next_message().await {
      if let ServerMessage::ClientResponse(client_response) = response {
        if client_response.client_message == client_message {
          return Some(client_response);
        }
      }
    }

    None
  }
}

impl Drop for LocalIpcClient {
  fn drop(&mut self) {
    // Mirrors a socket closing, so that any subscriptions this client
    // opened are torn down rather than left running.
    let _ = self.disconnection_tx.send(());
  }
}

pub struct IpcServer {
  abort_handle: task::AbortHandle,
  pub message_rx: mpsc::Receiver<MessageChannel>,
  _event_rx: broadcast::Receiver<(SubscribableEvent, WmEvent)>,
  event_tx: broadcast::Sender<(SubscribableEvent, WmEvent)>,
  _unsubscribe_rx: broadcast::Receiver<Uuid>,
  unsubscribe_tx: broadcast::Sender<Uuid>,
}

impl IpcServer {
  pub fn start() -> anyhow::Result<Self> {
    let (message_tx, message_rx) = mpsc::channel(MESSAGE_BUFFER_SIZE);
    let (event_tx, _event_rx) = broadcast::channel(16);
    let (unsubscribe_tx, _unsubscribe_rx) = broadcast::channel(16);

    // A named pipe / Unix socket rather than a TCP port. Access is
    // restricted to the current user by the OS, so no webpage and no other
    // user's process can reach it.
    let mut listener = IpcListener::bind()?;

    // Publish the inbound channel so in-process clients can reach the
    // server without a socket round-trip.
    *LOCAL_MESSAGE_TX
      .lock()
      .map_err(|_| anyhow::anyhow!("IPC server lock was poisoned."))? =
      Some(message_tx.clone());

    info!("IPC server started.");

    let connections = Arc::new(Semaphore::new(MAX_CONNECTIONS));

    let task = task::spawn(async move {
      loop {
        // A failed accept (e.g. the client vanished immediately) must not
        // take down the whole listener.
        let stream = match listener.accept().await {
          Ok(stream) => stream,
          Err(err) => {
            warn!("Failed to accept IPC connection: {}", err);
            continue;
          }
        };

        // Reject the connection outright when at capacity, rather than
        // spawning an unbounded number of tasks.
        let Ok(permit) = connections.clone().try_acquire_owned() else {
          warn!("Rejecting IPC connection: too many open connections.");
          drop(stream);
          continue;
        };

        let message_tx = message_tx.clone();

        task::spawn(async move {
          if let Err(err) =
            Self::handle_connection(stream, message_tx).await
          {
            warn!("Error handling connection: {}", err);
          }

          // Free the connection slot for the next client.
          drop(permit);
        });
      }
    });

    Ok(Self {
      abort_handle: task.abort_handle(),
      #[allow(clippy::used_underscore_binding)]
      _event_rx,
      event_tx,
      message_rx,
      unsubscribe_tx,
      #[allow(clippy::used_underscore_binding)]
      _unsubscribe_rx,
    })
  }

  async fn handle_connection(
    stream: IpcStream,
    message_tx: mpsc::Sender<MessageChannel>,
  ) -> anyhow::Result<()> {
    info!("Incoming IPC connection.");

    let (reader, mut writer) = stream.split();
    let mut reader = buf_reader(reader);

    // Annotated explicitly: inference would otherwise settle on `str`
    // from the `&str` that `write_message` takes.
    let (response_tx, mut response_rx) =
      mpsc::channel::<String>(RESPONSE_BUFFER_SIZE);
    let (disconnection_tx, _) = broadcast::channel(16);

    // Writing runs on its own task rather than in a `select!` alongside
    // the read. `read_message` is not cancel-safe: dropping it midway
    // would discard bytes it had already consumed from the buffer and
    // desynchronise the stream.
    let writer_task = task::spawn(async move {
      while let Some(response) = response_rx.recv().await {
        if let Err(err) = write_message(&mut writer, &response).await {
          warn!("Failed to write IPC response: {}", err);
          break;
        }
      }
    });

    let res = async {
      loop {
        match read_message(&mut reader).await? {
          Some(message) => {
            // Awaited so that a flooding client is slowed by
            // backpressure instead of growing the queue.
            message_tx
              .send((
                message,
                response_tx.clone(),
                disconnection_tx.clone(),
              ))
              .await?;
          }
          // Client disconnected.
          None => break Ok(()),
        }
      }
    }
    .await;

    writer_task.abort();

    info!("IPC disconnection.");

    // A send error here only means nobody was listening, which is the
    // normal case: a client that ran one command and left never
    // subscribed to anything. Only worth noting at debug.
    if let Err(err) = disconnection_tx.send(()) {
      debug!("Nothing was subscribed to this connection: {}", err);
    }

    res
  }

  pub fn process_message(
    &self,
    message: String,
    response_tx: &mpsc::Sender<String>,
    disconnection_tx: &broadcast::Sender<()>,
    wm: &mut WindowManager,
    config: &mut UserConfig,
  ) -> anyhow::Result<()> {
    let app_command = AppCommand::try_parse_from(
      iter::once("").chain(message.split_whitespace()),
    );

    let response_data =
      app_command
        .map_err(anyhow::Error::msg)
        .and_then(|app_command| {
          self.handle_app_command(
            app_command,
            response_tx,
            disconnection_tx,
            wm,
            config,
          )
        });

    // Respond to the client with the result of the command. Uses
    // `try_send` since this runs on the main loop, which must never block
    // on a slow client.
    response_tx
      .try_send(Self::to_client_response_msg(message, response_data)?)
      .map_err(|err| anyhow::anyhow!("Failed to send response: {err}"))?;

    Ok(())
  }

  #[allow(clippy::too_many_lines)]
  fn handle_app_command(
    &self,
    app_command: AppCommand,
    response_tx: &mpsc::Sender<String>,
    disconnection_tx: &broadcast::Sender<()>,
    wm: &mut WindowManager,
    config: &mut UserConfig,
  ) -> anyhow::Result<ClientResponseData> {
    let response_data = match app_command {
      AppCommand::Query { command } => match command {
        QueryCommand::Windows => {
          ClientResponseData::Windows(WindowsData {
            windows: wm
              .state
              .windows()
              .into_iter()
              .map(|window| window.to_dto())
              .try_collect()?,
          })
        }
        QueryCommand::Workspaces => {
          ClientResponseData::Workspaces(WorkspacesData {
            workspaces: wm
              .state
              .workspaces()
              .into_iter()
              .map(|workspace| workspace.to_dto())
              .try_collect()?,
          })
        }
        QueryCommand::Monitors => {
          ClientResponseData::Monitors(MonitorsData {
            monitors: wm
              .state
              .monitors()
              .into_iter()
              .map(|monitor| monitor.to_dto())
              .try_collect()?,
          })
        }
        QueryCommand::BindingModes => {
          ClientResponseData::BindingModes(BindingModesData {
            binding_modes: wm.state.binding_modes.clone(),
          })
        }
        QueryCommand::Focused => {
          let focused_container = wm
            .state
            .focused_container()
            .context("No focused container.")?;

          ClientResponseData::Focused(FocusedData {
            focused: focused_container.to_dto()?,
          })
        }
        QueryCommand::AppMetadata => {
          ClientResponseData::AppMetadata(AppMetadataData {
            version: env!("VERSION_NUMBER").to_string(),
          })
        }
        QueryCommand::ConfigPath => {
          ClientResponseData::ConfigPath(ConfigPathData {
            config_path: config.path.to_string_lossy().to_string(),
          })
        }
        QueryCommand::TilingDirection => {
          let direction_container = wm
            .state
            .focused_container()
            .and_then(|focused| focused.direction_container())
            .context("No direction container.")?;

          ClientResponseData::TilingDirection(TilingDirectionData {
            direction_container: direction_container.to_dto()?,
            tiling_direction: direction_container.tiling_direction(),
          })
        }
        QueryCommand::Paused => {
          ClientResponseData::Paused(wm.state.is_paused)
        }
      },
      AppCommand::Command {
        subject_container_id,
        command,
      } => {
        let subject_container_id = wm.process_commands(
          &vec![command],
          subject_container_id,
          config,
        )?;

        ClientResponseData::Command(CommandData {
          subject_container_id,
        })
      }
      AppCommand::Sub { events } => {
        let subscription_id = Uuid::new_v4();
        info!("New event subscription {}: {:?}", subscription_id, events);

        let response_tx = response_tx.clone();
        let mut event_rx = self.event_tx.subscribe();
        let mut unsubscribe_rx = self.unsubscribe_tx.subscribe();
        let mut disconnection_rx = disconnection_tx.subscribe();

        task::spawn(async move {
          loop {
            tokio::select! {
              Ok(()) = disconnection_rx.recv() => {
                break;
              }
              Ok(id) = unsubscribe_rx.recv() => {
                if id == subscription_id {
                  break;
                }
              }
              Ok((event_type, event)) = event_rx.recv() => {
                // Check whether the event is one of the subscribed events.
                if events.contains(&event_type)
                  || events.contains(&SubscribableEvent::All)
                {
                  // `try_send` rather than `send`, so that a client which
                  // stops reading doesn't buffer events without limit.
                  let event_msg = Self::to_event_subscription_msg(
                    subscription_id,
                    event,
                  );

                  match event_msg
                    .map(|event_msg| response_tx.try_send(event_msg))
                  {
                    // Shed the event and keep the subscription. Events are
                    // deltas and clients re-query full state anyway, so
                    // dropping a few is harmless; silently unsubscribing a
                    // live client is not. The queue fills during bursts
                    // (a display wake fires an event per window), which is
                    // exactly when the client still needs to be told.
                    Ok(Err(TrySendError::Full(_))) => {
                      debug!(
                        "Dropping event for subscription {}: queue full.",
                        subscription_id
                      );
                    }
                    // The connection is gone, so the subscription is too.
                    Ok(Err(TrySendError::Closed(_))) => break,
                    Err(err) => {
                      warn!("Error emitting WM event: {}", err);
                      break;
                    }
                    Ok(Ok(())) => {}
                  }
                }
              }
            }
          }
        });

        ClientResponseData::EventSubscribe(EventSubscribeData {
          subscription_id,
        })
      }
      AppCommand::Unsub { subscription_id } => {
        self
          .unsubscribe_tx
          .send(subscription_id)
          .context("Failed to unsubscribe from event.")?;

        ClientResponseData::EventUnsubscribe
      }
      AppCommand::Start { .. } => bail!("Unsupported IPC command."),
    };

    Ok(response_data)
  }

  fn to_client_response_msg(
    client_message: String,
    response_data: anyhow::Result<ClientResponseData>,
  ) -> anyhow::Result<String> {
    let error = response_data.as_ref().err().map(ToString::to_string);
    let success = response_data.as_ref().is_ok();

    let message = ServerMessage::ClientResponse(ClientResponseMessage {
      client_message,
      data: response_data.ok(),
      error,
      success,
    });

    Ok(serde_json::to_string(&message)?)
  }

  fn to_event_subscription_msg(
    subscription_id: Uuid,
    event: WmEvent,
  ) -> anyhow::Result<String> {
    let message =
      ServerMessage::EventSubscription(EventSubscriptionMessage {
        data: Some(event),
        error: None,
        subscription_id,
        success: true,
      });

    Ok(serde_json::to_string(&message)?)
  }

  pub fn process_event(&mut self, event: WmEvent) -> anyhow::Result<()> {
    let event_type = match event {
      WmEvent::ApplicationExiting => SubscribableEvent::ApplicationExiting,
      WmEvent::BindingModesChanged { .. } => {
        SubscribableEvent::BindingModesChanged
      }
      WmEvent::FocusChanged { .. } => SubscribableEvent::FocusChanged,
      WmEvent::FocusedContainerMoved { .. } => {
        SubscribableEvent::FocusedContainerMoved
      }
      WmEvent::MonitorAdded { .. } => SubscribableEvent::MonitorAdded,
      WmEvent::MonitorUpdated { .. } => SubscribableEvent::MonitorUpdated,
      WmEvent::MonitorRemoved { .. } => SubscribableEvent::MonitorRemoved,
      WmEvent::TilingDirectionChanged { .. } => {
        SubscribableEvent::TilingDirectionChanged
      }
      WmEvent::UserConfigChanged { .. } => {
        SubscribableEvent::UserConfigChanged
      }
      WmEvent::WindowManaged { .. } => SubscribableEvent::WindowManaged,
      WmEvent::WindowUnmanaged { .. } => {
        SubscribableEvent::WindowUnmanaged
      }
      WmEvent::WorkspaceActivated { .. } => {
        SubscribableEvent::WorkspaceActivated
      }
      WmEvent::WorkspaceDeactivated { .. } => {
        SubscribableEvent::WorkspaceDeactivated
      }
      WmEvent::WorkspaceUpdated { .. } => {
        SubscribableEvent::WorkspaceUpdated
      }
      WmEvent::PauseChanged { .. } => SubscribableEvent::PauseChanged,
    };

    self
      .event_tx
      .send((event_type, event))
      .map_err(|err| anyhow::anyhow!("Failed to send event: {err}"))?;

    Ok(())
  }

  pub fn stop(&self) {
    info!("Shutting down IPC server.");
    self.abort_handle.abort();

    // In-process clients connect through this, so clearing it is what
    // makes them fail to connect rather than queue into a dead server.
    if let Ok(mut message_tx) = LOCAL_MESSAGE_TX.lock() {
      *message_tx = None;
    }
  }
}

impl Drop for IpcServer {
  fn drop(&mut self) {
    self.stop();
  }
}
