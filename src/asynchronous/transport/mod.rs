use std::io::{Error as IoError, Result as IoResult};
use std::pin::Pin;
#[cfg(feature = "security_extension")]
use std::os::unix::io::RawFd;

use futures::stream::{BoxStream, Stream, StreamExt as _};
use tokio::io::{AsyncRead, AsyncWrite};

trait AsyncReadWrite: AsyncRead + AsyncWrite {}
impl<T: AsyncRead + AsyncWrite> AsyncReadWrite for T {}

pub struct Listener(BoxStream<'static, IoResult<Socket>>);
/// A type-erased async socket.
///
/// On Unix with the `security_extension` feature, stores the raw fd for
/// [`AcceptHook`](crate::security_extension::AcceptHook) access.
/// See `crate::security_extension` module doc for full architecture.
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
    /// Create a listener from a generic async stream of sockets.
    ///
    /// **Note**: This uses [`Socket::new`] which does **not** capture the
    /// raw fd. For platform-specific listeners (Unix, TCP, vsock), prefer
    /// the `From<XxxListener>` impls which capture the raw fd via
    /// `Socket::from()` — required by [`AcceptHook`](crate::security_extension::AcceptHook).
    pub fn new<S: AsyncRead + AsyncWrite + Send + Sync + 'static>(
        listener: impl Stream<Item = IoResult<S>> + Send + 'static,
    ) -> Self {
        Self(listener.map(|s| s.map(Socket::new)).boxed())
    }

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
    /// Create a socket from any `AsyncRead + AsyncWrite` type.
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
    pub fn as_raw_fd(&self) -> Option<RawFd> {
        self.raw_fd
    }

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
