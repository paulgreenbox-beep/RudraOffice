//! Lightweight Office entry screen and typed process boundary. No document engines.
pub mod protocol;
pub use protocol::{DocumentKind, LaunchRequest, parse_editor_args, sibling_editor_path};
#[cfg(all(feature = "gui", windows))]
pub mod accessibility;
#[cfg(feature = "gui")]
pub mod view;
