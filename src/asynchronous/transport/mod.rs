//! Asynchronous transport abstraction for ttrpc connections.
//!
//! [`Listener`](crate::asynchronous::transport::Listener) yields accepted
//! [`Socket`](crate::asynchronous::transport::Socket) values, and `Socket` implements Tokio's
//! [`AsyncRead`](tokio::io::AsyncRead) and [`AsyncWrite`](tokio::io::AsyncWrite) traits. Use
//! [`Listener::bind`](crate::asynchronous::transport::Listener::bind) and
//! [`Socket::connect`](crate::asynchronous::transport::Socket::connect) for built-in address
//! schemes, or [`Listener::new`](crate::asynchronous::transport::Listener::new) and
//! [`Socket::new`](crate::asynchronous::transport::Socket::new) to integrate a custom byte-stream
//! transport.

use std::io::{Error as IoError, Result as IoResult};
use std::pin::Pin;
#[cfg(feature = "security_extension")]
use std::os::unix::io::RawFd;

use futures::stream::{BoxStream, Stream, StreamExt as _};
use tokio::io::{AsyncRead, AsyncWrite};

trait AsyncReadWrite: AsyncRead + AsyncWrite {}
impl<T: AsyncRead + AsyncWrite> AsyncReadWrite for T {}

/// A stream of accepted asynchronous ttrpc connections.
pub struct Listener(BoxStream<'static, IoResult<Socket>>);
/// A type-erased asynchronous byte stream used by the ttrpc runtime.
///
/// On Unix with the `security_extension` feature, stores the raw fd for `AcceptHook` access.
pub struct Socket {
    inner: Pin<Box<dyn AsyncReadWrite + Send + Sync + 'static>>,
    #[cfg(feature = "security_extension")]
    raw_fd: Option<RawFd>,
}

macro_rules! io_other {
    ($fmt_str:literal, $($args:expr),*) => {
        IoError::new(std::io::ErrorKind::Other, format!($fmt_str, $($args),*))
    };
    ($fmt_str:literal) => {
        IoError::new(std::io::ErrorKind::Other, format!($fmt_str))
    };
}

#[cfg(unix)]
mod unix;

#[cfg(unix)]
mod tcp;

#[cfg(any(target_os = "linux", target_os = "android"))]
mod vsock;

#[cfg(windows)]
mod windows;

impl Listener {
    /// Creates a listener from a stream of accepted asynchronous I/O values.
    ///
    /// This is the extension point for custom transports. Each successful item is wrapped as a
    /// [`Socket`]; listener errors are preserved. Custom transports do not expose a raw file
    /// descriptor, so they cannot be used with `AcceptHook`. Platform listeners created through
    /// [`Listener::bind`] do expose their descriptor.
    pub fn new<S: AsyncRead + AsyncWrite + Send + Sync + 'static>(
        listener: impl Stream<Item = IoResult<S>> + Send + 'static,
    ) -> Self {
        Self(listener.map(|s| s.map(Socket::new)).boxed())
    }

    /// Binds a built-in transport address.
    ///
    /// See the [crate-level transport table](crate#transport-addresses) for supported schemes and
    /// platforms.
    ///
    /// # Errors
    ///
    /// Returns an error if the address scheme is unsupported, the address is invalid, or the
    /// operating system cannot create and bind the listener.
    pub fn bind(addr: impl AsRef<str>) -> std::io::Result<Self> {
        let addr = addr.as_ref();

        #[cfg(unix)]
        if let Some(addr) = addr.strip_prefix("unix://") {
            return Self::bind_unix(addr);
        }

        #[cfg(unix)]
        if let Some(addr) = addr.strip_prefix("tcp://") {
            return Self::bind_tcp(addr);
        }

        #[cfg(any(target_os = "linux", target_os = "android"))]
        if let Some(addr) = addr.strip_prefix("vsock://") {
            return Self::bind_vsock(addr);
        }

        #[cfg(windows)]
        if addr.starts_with(r"\\.\pipe\") {
            return Self::bind_named_pipe(addr);
        }

        Err(io_other!("Scheme of {addr:?} is not supported"))
    }
}

impl Socket {
    /// Wraps an asynchronous reader/writer as a ttrpc socket.
    ///
    /// This is the extension point for custom connected transports. The resulting socket does not
    /// expose a raw file descriptor and therefore cannot be used with `AcceptHook`.
    pub fn new(socket: impl AsyncRead + AsyncWrite + Send + Sync + 'static) -> Self {
        Self {
            inner: Box::pin(socket),
            #[cfg(feature = "security_extension")]
            raw_fd: None,
        }
    }

    #[cfg(feature = "security_extension")]
    pub(crate) fn with_raw_fd(socket: impl AsyncRead + AsyncWrite + Send + Sync + 'static, fd: RawFd) -> Self {
        Self {
            inner: Box::pin(socket),
            raw_fd: Some(fd),
        }
    }

    #[cfg(feature = "security_extension")]
    /// Returns the socket's borrowed raw file descriptor when one was captured.
    ///
    /// Built-in Unix transports retain their descriptor for connection hooks. Sockets created by
    /// [`Socket::new`] return `None` because type erasure does not expose an underlying descriptor.
    pub fn as_raw_fd(&self) -> Option<RawFd> {
        self.raw_fd
    }

    /// Create a socket from a stream, capturing the raw fd on Unix when
    /// `security_extension` is enabled. Eliminates per-transport cfg branching
    /// in `From<XxxStream> for Socket` impls.
    #[cfg(unix)]
    pub(crate) fn from_fd_aware<S: std::os::unix::io::AsRawFd + AsyncRead + AsyncWrite + Send + Sync + 'static>(
        socket: S,
    ) -> Self {
        #[cfg(feature = "security_extension")]
        {
            let fd = socket.as_raw_fd();
            Self::with_raw_fd(socket, fd)
        }
        #[cfg(not(feature = "security_extension"))]
        {
            Self::new(socket)
        }
    }

    /// Connects to a built-in transport address.
    ///
    /// See the [crate-level transport table](crate#transport-addresses) for supported schemes and
    /// platforms.
    ///
    /// # Errors
    ///
    /// Returns an error if the address scheme is unsupported, the address is invalid, or the
    /// connection cannot be established.
    pub async fn connect(addr: impl AsRef<str>) -> IoResult<Self> {
        let addr = addr.as_ref();

        #[cfg(unix)]
        if let Some(addr) = addr.strip_prefix("unix://") {
            return Self::connect_unix(addr).await;
        }

        #[cfg(unix)]
        if let Some(addr) = addr.strip_prefix("tcp://") {
            return Self::connect_tcp(addr).await;
        }

        #[cfg(any(target_os = "linux", target_os = "android"))]
        if let Some(addr) = addr.strip_prefix("vsock://") {
            return Self::connect_vsock(addr).await;
        }

        #[cfg(windows)]
        if addr.starts_with(r"\\.\pipe\") {
            return Self::connect_named_pipe(addr).await;
        }

        Err(io_other!("Scheme of {addr:?} is not supported"))
    }
}

impl Stream for Listener {
    type Item = IoResult<Socket>;

    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.get_mut().0.as_mut().poll_next(cx)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}

impl AsyncRead for Socket {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        self.get_mut().inner.as_mut().poll_read(cx, buf)
    }
}

impl AsyncWrite for Socket {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<Result<usize, std::io::Error>> {
        self.get_mut().inner.as_mut().poll_write(cx, buf)
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        self.get_mut().inner.as_mut().poll_flush(cx)
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        self.get_mut().inner.as_mut().poll_shutdown(cx)
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        bufs: &[std::io::IoSlice<'_>],
    ) -> std::task::Poll<Result<usize, std::io::Error>> {
        self.get_mut().inner.as_mut().poll_write_vectored(cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }
}
