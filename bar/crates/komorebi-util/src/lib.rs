// `CLAUDE.md` requires a `SAFETY:` comment on every `unsafe` block.
#![warn(clippy::undocumented_unsafe_blocks)]

mod client;
mod error;
mod types;

pub use client::*;
pub use error::*;
pub use types::*;

pub type Result<T> = std::result::Result<T, Error>;
