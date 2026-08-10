use std::convert::TryFrom;
use std::io::{Error as IoError, Result as IoResult};
#[cfg(feature = "security_extension")]
use std::os::fd::AsRawFd as _;
use std::os::fd::{FromRawFd as _, RawFd};
use std::os::unix::net::{
    SocketAddr, UnixListener as StdUnixListener, UnixStream as StdUnixStream,
};

use async_stream::stream;
use tokio::net::{UnixListener, UnixStream};

use super::{Listener, Socket};

impl Listener {
    pub fn bind_unix(addr: impl AsRef<str>) -> IoResult<Self> {
        let addr = parse_unix_addr(addr)?;
        let listener = StdUnixListener::bind_addr(&addr)?;
        Self::try_from(listener)
    }

    /// # Safety
    /// The file descriptor must represent a unix listener.
    pub unsafe fn from_raw_unix_listener_fd(fd: std::os::fd::RawFd) -> IoResult<Self> {
        let listener = unsafe { StdUnixListener::from_raw_fd(fd) };
        Self::try_from(listener)
    }
}

impl Socket {
    pub async fn connect_unix(addr: impl AsRef<str>) -> IoResult<Self> {
        let addr = parse_unix_addr(addr)?;
        let socket = StdUnixStream::connect_addr(&addr)?;
        Self::try_from(socket)
    }

    /// # Safety
    /// The file descriptor must represent a unix socket.
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
        #[cfg(feature = "security_extension")]
        {
            let fd = socket.as_raw_fd();
            Socket::with_raw_fd(socket, fd)
        }
        #[cfg(not(feature = "security_extension"))]
        {
            Socket::new(socket)
        }
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
