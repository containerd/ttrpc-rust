use std::io::{Error as IoError, Result as IoResult};
use std::os::fd::{FromRawFd as _, RawFd};

use async_stream::stream;
use tokio_vsock::{VsockAddr, VsockListener, VsockStream, VMADDR_CID_ANY};

use super::{Listener, Socket};

impl Listener {
    pub fn bind_vsock(addr: impl AsRef<str>) -> IoResult<Self> {
        let addr = parse_vsock_addr(addr)?;
        Ok(Self::from(VsockListener::bind(addr)?))
    }

    /// # Safety
    /// The file descriptor must represent a vsock listener.
    pub unsafe fn from_raw_vsock_listener_fd(fd: RawFd) -> IoResult<Self> {
        let listener = unsafe { VsockListener::from_raw_fd(fd) };
        Ok(Self::from(listener))
    }
}

impl Socket {
    pub async fn connect_vsock(addr: impl AsRef<str>) -> IoResult<Self> {
        let addr = parse_vsock_addr(addr)?;
        Ok(Self::from(VsockStream::connect(addr).await?))
    }
}

impl From<VsockListener> for Listener {
    fn from(listener: VsockListener) -> Self {
        Self::new(stream! {
            loop {
                yield listener.accept().await.map(|(socket, _)| socket);
            }
        })
    }
}

impl From<VsockStream> for Socket {
    fn from(socket: VsockStream) -> Self {
        Self::new(socket)
    }
}

fn parse_vsock_addr(addr: impl AsRef<str>) -> IoResult<VsockAddr> {
    let addr = addr.as_ref();

    let addr_parts: Vec<&str> = addr.split(':').collect();
    let [cid, port] = addr_parts[..] else {
        return Err(io_other!("sockaddr {addr} is not right for vsock"));
    };

    // for -1 need trace to libc::VMADDR_CID_ANY
    let cid: u32 = if cid.trim().eq("-1") {
        VMADDR_CID_ANY
    } else {
        cid.parse()
            .map_err(|e| io_other!("failed to parse cid from {cid:?} error: {e:?}"))?
    };

    let port: u32 = port
        .parse()
        .map_err(|e| io_other!("failed to parse port from {port:?} error: {e:?}"))?;

    Ok(VsockAddr::new(cid, port))
}
