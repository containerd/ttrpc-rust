#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use crate::sync::sys::unix::{PipeConnection, PipeListener, ClientConnection};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use crate::sync::sys::windows::{PipeConnection, PipeListener, ClientConnection};
