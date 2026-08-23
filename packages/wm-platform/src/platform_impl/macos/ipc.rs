use std::{
  fs,
  os::unix::fs::{DirBuilderExt, PermissionsExt},
  path::PathBuf,
};

use tokio::{
  io::split,
  net::{UnixListener, UnixStream},
};

use crate::{IpcReader, IpcWriter};

/// Returns the path of the IPC socket.
///
/// The socket lives under the user's home directory, so it is inherently
/// per-user and two accounts can't collide.
fn socket_path() -> crate::Result<PathBuf> {
  let path = home::home_dir()
    .ok_or_else(|| {
      crate::Error::Platform("Unable to get home directory.".to_string())
    })?
    .join(".ninja/ipc.sock");

  Ok(path)
}

/// Platform-specific implementation of [`IpcListener`].
pub struct IpcListener {
  listener: UnixListener,
}

impl IpcListener {
  /// Implements [`IpcListener::bind`].
  pub(crate) fn bind() -> crate::Result<Self> {
    let path = socket_path()?;

    let parent_dir = path.parent().ok_or_else(|| {
      crate::Error::Platform("Invalid IPC socket path.".to_string())
    })?;

    // Created as `0700` so that the socket is unreachable by other users
    // even in the instant before its own permissions are set below.
    fs::DirBuilder::new()
      .recursive(true)
      .mode(0o700)
      .create(parent_dir)?;

    // Remove a stale socket left behind by a previous crash, which would
    // otherwise make `bind` fail with "address in use".
    if path.exists() {
      fs::remove_file(&path)?;
    }

    let listener = UnixListener::bind(&path)?;

    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;

    Ok(Self { listener })
  }

  /// Implements [`IpcListener::accept`].
  pub(crate) async fn accept(
    &mut self,
  ) -> crate::Result<(IpcReader, IpcWriter)> {
    let (stream, _) = self.listener.accept().await?;

    let (reader, writer) = split(stream);
    Ok((Box::new(reader), Box::new(writer)))
  }
}

/// Implements [`IpcStream::connect`].
pub(crate) async fn connect() -> crate::Result<(IpcReader, IpcWriter)> {
  let stream = UnixStream::connect(socket_path()?).await?;

  let (reader, writer) = split(stream);
  Ok((Box::new(reader), Box::new(writer)))
}
