use std::io::{Error as IoError, Result as IoResult};
use std::pin::Pin;

use futures::stream::{BoxStream, Stream, StreamExt as _};
use tokio::io::{AsyncRead, AsyncWrite};

trait AsyncReadWrite: AsyncRead + AsyncWrite {}
impl<T: AsyncRead + AsyncWrite> AsyncReadWrite for T {}

pub struct Listener(BoxStream<'static, IoResult<Socket>>);
pub struct Socket(Pin<Box<dyn AsyncReadWrite + Send + Sync + 'static>>);

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
    pub fn new(socket: impl AsyncRead + AsyncWrite + Send + Sync + 'static) -> Self {
        Self(Box::pin(socket))
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
        self.get_mut().0.as_mut().poll_read(cx, buf)
    }
}

impl AsyncWrite for Socket {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<Result<usize, std::io::Error>> {
        self.get_mut().0.as_mut().poll_write(cx, buf)
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        self.get_mut().0.as_mut().poll_flush(cx)
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        self.get_mut().0.as_mut().poll_shutdown(cx)
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        bufs: &[std::io::IoSlice<'_>],
    ) -> std::task::Poll<Result<usize, std::io::Error>> {
        self.get_mut().0.as_mut().poll_write_vectored(cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        self.0.is_write_vectored()
    }
}
