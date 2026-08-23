use tokio::io::{
  AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader,
};

use crate::platform_impl;

/// Read half of an IPC connection.
pub type IpcReader = Box<dyn AsyncRead + Send + Unpin>;

/// Write half of an IPC connection.
pub type IpcWriter = Box<dyn AsyncWrite + Send + Unpin>;

/// Maximum length of a single IPC message, in bytes.
///
/// Caps how much a client can make the receiver buffer before a message is
/// complete.
pub const MAX_MESSAGE_LEN: usize = 64 * 1024;

/// A listener for incoming IPC connections.
///
/// Access is restricted to the current user by the operating system, so
/// there is no network socket and no application-level handshake.
///
/// # Platform-specific
///
/// - **Windows**: A named pipe at `\\.\pipe\ninja-ipc-<user>`, with a DACL
///   granting access only to the owning user and `LocalSystem`.
/// - **macOS**: A Unix domain socket at `~/.ninja/ipc.sock`, with `0600`
///   permissions.
pub struct IpcListener {
  inner: platform_impl::IpcListener,
}

impl IpcListener {
  /// Creates an [`IpcListener`], ready to accept connections.
  pub fn bind() -> crate::Result<Self> {
    Ok(Self {
      inner: platform_impl::IpcListener::bind()?,
    })
  }

  /// Waits for the next client and returns its connection.
  pub async fn accept(&mut self) -> crate::Result<IpcStream> {
    let (reader, writer) = self.inner.accept().await?;
    Ok(IpcStream { reader, writer })
  }
}

/// A connected IPC stream.
pub struct IpcStream {
  reader: IpcReader,
  writer: IpcWriter,
}

impl IpcStream {
  /// Connects to the IPC listener of the running WM instance.
  pub async fn connect() -> crate::Result<Self> {
    let (reader, writer) = platform_impl::connect().await?;
    Ok(Self { reader, writer })
  }

  /// Splits the stream into its read and write halves.
  #[must_use]
  pub fn split(self) -> (IpcReader, IpcWriter) {
    (self.reader, self.writer)
  }
}

/// Reads a single newline-delimited message.
///
/// Returns `None` when the peer has disconnected.
///
/// # Errors
///
/// Returns [`Error::Platform`] if the message exceeds
/// [`MAX_MESSAGE_LEN`] or is not valid UTF-8.
pub async fn read_message<R>(
  reader: &mut BufReader<R>,
) -> crate::Result<Option<String>>
where
  R: AsyncRead + Unpin,
{
  let mut message = Vec::new();

  loop {
    let mut consumed = 0;
    let mut is_complete = false;
    let mut is_eof = false;

    {
      let available = reader.fill_buf().await?;

      if available.is_empty() {
        is_eof = true;
      } else if let Some(index) =
        available.iter().position(|&byte| byte == b'\n')
      {
        message.extend_from_slice(&available[..index]);
        consumed = index + 1;
        is_complete = true;
      } else {
        message.extend_from_slice(available);
        consumed = available.len();
      }
    }

    reader.consume(consumed);

    if is_eof {
      // A trailing message without a newline is discarded, since it can't
      // be distinguished from a truncated one.
      return Ok(None);
    }

    if is_complete {
      let message = String::from_utf8(message).map_err(|_| {
        crate::Error::Platform(
          "IPC message is not valid UTF-8.".to_string(),
        )
      })?;

      return Ok(Some(message));
    }

    // Checked only after consuming, so that an oversized message is
    // rejected rather than buffered indefinitely.
    if message.len() > MAX_MESSAGE_LEN {
      return Err(crate::Error::Platform(format!(
        "IPC message exceeds {MAX_MESSAGE_LEN} bytes."
      )));
    }
  }
}

/// Writes a single newline-delimited message.
///
/// The message must not contain a newline. Command strings and
/// `serde_json` output never do.
pub async fn write_message<W>(
  writer: &mut W,
  message: &str,
) -> crate::Result<()>
where
  W: AsyncWrite + Unpin,
{
  writer.write_all(message.as_bytes()).await?;
  writer.write_all(b"\n").await?;
  writer.flush().await?;

  Ok(())
}

/// Wraps a reader for use with [`read_message`].
pub fn buf_reader<R>(reader: R) -> BufReader<R>
where
  R: AsyncRead + Unpin,
{
  BufReader::new(reader)
}
