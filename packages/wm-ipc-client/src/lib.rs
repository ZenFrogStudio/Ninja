#![allow(clippy::missing_errors_doc)]

use anyhow::Context;
use tokio::io::BufReader;
use uuid::Uuid;
use wm_common::{
  ClientResponseMessage, EventSubscriptionMessage, ServerMessage,
};
use wm_platform::{
  buf_reader, read_message, write_message, IpcReader, IpcStream, IpcWriter,
};

pub struct IpcClient {
  reader: BufReader<IpcReader>,
  writer: IpcWriter,
}

impl IpcClient {
  /// Connects to the IPC listener of the running WM instance.
  ///
  /// Access is enforced by the operating system, so there is no
  /// application-level handshake.
  pub async fn connect() -> anyhow::Result<Self> {
    let stream = IpcStream::connect()
      .await
      .context("Failed to connect to IPC server. Is Ninja running?")?;

    let (reader, writer) = stream.split();

    Ok(Self {
      reader: buf_reader(reader),
      writer,
    })
  }

  /// Sends a message to the IPC server.
  pub async fn send(&mut self, message: &str) -> anyhow::Result<()> {
    write_message(&mut self.writer, message)
      .await
      .context("Failed to send command.")?;

    Ok(())
  }

  /// Waits and returns the next reply from the IPC server.
  pub async fn next_message(&mut self) -> anyhow::Result<ServerMessage> {
    let response = read_message(&mut self.reader)
      .await
      .context("Failed to receive response.")?
      .context("IPC connection closed.")?;

    let json_response = serde_json::from_str::<ServerMessage>(&response)?;

    Ok(json_response)
  }

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

  pub async fn event_subscription(
    &mut self,
    subscription_id: &Uuid,
  ) -> Option<EventSubscriptionMessage> {
    while let Ok(response) = self.next_message().await {
      if let ServerMessage::EventSubscription(event_sub) = response {
        if &event_sub.subscription_id == subscription_id {
          return Some(event_sub);
        }
      }
    }

    None
  }
}
