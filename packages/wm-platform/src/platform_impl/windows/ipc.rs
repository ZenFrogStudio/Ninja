use std::{ffi::c_void, mem::size_of, time::Duration};

use tokio::{
  io::split,
  net::windows::named_pipe::{
    ClientOptions, NamedPipeServer, ServerOptions,
  },
};
use windows::{
  core::HSTRING,
  Win32::Security::{
    Authorization::{
      ConvertStringSecurityDescriptorToSecurityDescriptorW,
      SDDL_REVISION_1,
    },
    PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
  },
};

use crate::{IpcReader, IpcWriter};

/// Security descriptor applied to every pipe instance, in SDDL form.
///
/// - `D:P` - a protected DACL, so no entries are inherited.
/// - `(A;;GA;;;OW)` - full access for the object's owner, which is the
///   user that created the pipe.
/// - `(A;;GA;;;SY)` - full access for `LocalSystem`.
///
/// Everyone else is denied by omission. This matters because the *default*
/// descriptor for a named pipe grants read access to `Everyone` and to the
/// anonymous account, which would expose window titles to other users.
const PIPE_SDDL: &str = "D:P(A;;GA;;;OW)(A;;GA;;;SY)";

/// How long to wait between attempts when the pipe is busy.
const CONNECT_RETRY_INTERVAL: Duration = Duration::from_millis(50);

/// Number of times to retry connecting while all instances are busy.
const CONNECT_RETRY_COUNT: u32 = 20;

/// Returns the name of the IPC pipe for the current user.
///
/// The name is per-user so that two users logged into the same machine
/// don't collide, which would otherwise let one user's instance block the
/// other's from starting.
fn pipe_name() -> String {
  let user =
    std::env::var("USERNAME").unwrap_or_else(|_| "default".to_string());

  format!(r"\\.\pipe\ninja-ipc-{user}")
}

/// Builds the security descriptor described by [`PIPE_SDDL`].
///
/// Returns the descriptor as a `usize` so that it can be held across
/// threads. The allocation is intentionally never freed: it must outlive
/// every pipe instance created from it, and only one is made per process.
fn build_security_descriptor() -> crate::Result<usize> {
  let sddl = HSTRING::from(PIPE_SDDL);
  let mut descriptor = PSECURITY_DESCRIPTOR::default();

  // SAFETY: `sddl` is a valid null-terminated wide string, and
  // `descriptor` is a valid out-pointer for the resulting descriptor.
  unsafe {
    ConvertStringSecurityDescriptorToSecurityDescriptorW(
      &sddl,
      SDDL_REVISION_1,
      &raw mut descriptor,
      None,
    )
  }?;

  Ok(descriptor.0 as usize)
}

/// Creates a single pipe instance with the given security descriptor.
///
/// `is_first` must be `true` only for the very first instance, which
/// reserves the name and prevents another process from squatting it.
fn create_server(
  name: &str,
  descriptor: usize,
  is_first: bool,
) -> crate::Result<NamedPipeServer> {
  let mut attributes = SECURITY_ATTRIBUTES {
    nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>())?,
    lpSecurityDescriptor: descriptor as *mut c_void,
    bInheritHandle: false.into(),
  };

  // SAFETY: `attributes` is a fully initialised `SECURITY_ATTRIBUTES` that
  // outlives the call, and its descriptor is valid for the process
  // lifetime.
  let server = unsafe {
    ServerOptions::new()
      .first_pipe_instance(is_first)
      .create_with_security_attributes_raw(
        name,
        std::ptr::from_mut(&mut attributes).cast(),
      )
  }?;

  Ok(server)
}

/// Platform-specific implementation of [`IpcListener`].
pub struct IpcListener {
  name: String,
  descriptor: usize,

  /// The idle pipe instance that the next client will connect to.
  server: NamedPipeServer,
}

impl IpcListener {
  /// Implements [`IpcListener::bind`].
  pub(crate) fn bind() -> crate::Result<Self> {
    let name = pipe_name();
    let descriptor = build_security_descriptor()?;
    let server = create_server(&name, descriptor, true)?;

    Ok(Self {
      name,
      descriptor,
      server,
    })
  }

  /// Implements [`IpcListener::accept`].
  pub(crate) async fn accept(
    &mut self,
  ) -> crate::Result<(IpcReader, IpcWriter)> {
    self.server.connect().await?;

    // Hand the connected instance to the caller and put a fresh idle
    // instance in its place for the next client.
    let next = create_server(&self.name, self.descriptor, false)?;
    let connected = std::mem::replace(&mut self.server, next);

    let (reader, writer) = split(connected);
    Ok((Box::new(reader), Box::new(writer)))
  }
}

/// Implements [`IpcStream::connect`].
pub(crate) async fn connect() -> crate::Result<(IpcReader, IpcWriter)> {
  let name = pipe_name();
  let mut attempts = 0;

  let client = loop {
    match ClientOptions::new().open(&name) {
      Ok(client) => break client,
      Err(err) => {
        // All instances are momentarily busy. The server creates a
        // replacement immediately after each accept, so this clears
        // quickly.
        if attempts >= CONNECT_RETRY_COUNT {
          return Err(err.into());
        }

        attempts += 1;
        tokio::time::sleep(CONNECT_RETRY_INTERVAL).await;
      }
    }
  };

  let (reader, writer) = split(client);
  Ok((Box::new(reader), Box::new(writer)))
}
