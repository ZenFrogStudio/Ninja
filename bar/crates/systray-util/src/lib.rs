// `CLAUDE.md` requires a `SAFETY:` comment on every `unsafe` block.
#![warn(clippy::undocumented_unsafe_blocks)]

mod error;
mod systray;
mod tray_spy;
mod util;

pub use error::*;
pub use systray::*;
pub(crate) use tray_spy::*;
pub(crate) use util::*;

pub type Result<T> = std::result::Result<T, Error>;
