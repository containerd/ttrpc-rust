use std::convert::TryFrom;
use std::io::{Error as IoError, Result as IoResult};
use std::os::fd::{FromRawFd as _, RawFd};
use std::os::unix::net::{
    SocketAddr, UnixListener as StdUnixListener, UnixStream as StdUnixStream,
};

use async_stream::stream;
use tokio::net::{UnixListener, UnixStream};

use super::{Listener, Socket};

impl Listener {
    /// Binds a Unix domain socket address without the `unix://` scheme prefix.
    ///
    /// On Linux and Android, an address beginning with `@` selects the abstract namespace.
    ///
    /// # Errors
    ///
    /// Returns an error if the address is invalid, already in use, or cannot be configured for
    /// asynchronous I/O.
    pub fn bind_unix(addr: impl AsRef<str>) -> IoResult<Self> {
        let addr = parse_unix_addr(addr)?;
        let listener = StdUnixListener::bind_addr(&addr)?;
        Self::try_from(listener)
    }

    /// Creates a listener from an existing Unix socket descriptor.
    ///
    /// # Safety
    ///
    /// `fd` must be a valid, open Unix listener. The caller must transfer exclusive ownership and
    /// must not close or use the descriptor afterward.
    ///
    /// # Errors
    ///
    /// Returns an error if the descriptor cannot be configured for asynchronous I/O.
    pub unsafe fn from_raw_unix_listener_fd(fd: std::os::fd::RawFd) -> IoResult<Self> {
        let listener = unsafe { StdUnixListener::from_raw_fd(fd) };
        Self::try_from(listener)
    }
}

impl Socket {
    /// Connects to a Unix domain socket address without the `unix://` scheme prefix.
    ///
    /// # Errors
    ///
    /// Returns an error if the address is invalid or the connection cannot be established.
    pub async fn connect_unix(addr: impl AsRef<str>) -> IoResult<Self> {
        let addr = parse_unix_addr(addr)?;
        let socket = StdUnixStream::connect_addr(&addr)?;
        Self::try_from(socket)
    }

    /// Creates a connected socket from an existing Unix socket descriptor.
    ///
    /// # Safety
    ///
    /// `fd` must be a valid, open, connected Unix socket. The caller must transfer exclusive
    /// ownership and must not close or use the descriptor afterward.
    ///
    /// # Errors
    ///
    /// Returns an error if the descriptor cannot be configured for asynchronous I/O.
    pub unsafe fn from_raw_unix_socket_fd(fd: RawFd) -> IoResult<Self> {
        let socket = unsafe { StdUnixStream::from_raw_fd(fd) };
        Self::try_from(socket)
    }
}

impl From<UnixListener> for Listener {
    /// Convert a `UnixListener` into a `Listener`.
    ///
    /// Uses `Box::pin(stream! {...})` directly (bypassing `Listener::new()`)
    /// so each accepted connection goes through `Socket::from(socket)`,
    /// which captures the raw fd when the `security_extension` feature is enabled.
    fn from(listener: UnixListener) -> Self {
        Self(Box::pin(stream! {
            loop {
                match listener.accept().await {
                    Ok((socket, _)) => yield Ok(Socket::from(socket)),
                    Err(e) => yield Err(e),
                }
            }
        }))
    }
}

impl TryFrom<StdUnixListener> for Listener {
    type Error = IoError;
    fn try_from(listener: StdUnixListener) -> IoResult<Self> {
        listener.set_nonblocking(true)?;
        Ok(Self::from(UnixListener::from_std(listener)?))
    }
}

impl From<UnixStream> for Socket {
    fn from(socket: UnixStream) -> Self {
        Socket::from_fd_aware(socket)
    }
}

impl TryFrom<StdUnixStream> for Socket {
    type Error = IoError;
    fn try_from(socket: StdUnixStream) -> IoResult<Self> {
        socket.set_nonblocking(true)?;
        Ok(Self::from(UnixStream::from_std(socket)?))
    }
}

fn parse_unix_addr(addr: impl AsRef<str>) -> IoResult<SocketAddr> {
    let addr = addr.as_ref();

    #[cfg(any(target_os = "linux", target_os = "android"))]
    if let Some(addr) = addr.strip_prefix('@') {
        use std::os::linux::net::SocketAddrExt as _;
        return SocketAddr::from_abstract_name(addr);
    }

    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    if addr.starts_with('@') {
        return Err(io_other!(
            "Abstract unix domain socket is not support on this platform",
        ));
    }

    SocketAddr::from_pathname(addr)
}
