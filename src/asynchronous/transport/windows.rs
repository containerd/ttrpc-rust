use std::io::Result as IoResult;
use std::mem::replace;
use std::time::Duration;

use async_stream::stream;
use tokio::net::windows::named_pipe::{
    ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions,
};
use tokio::time::sleep;
use windows_sys::Win32::Foundation::ERROR_PIPE_BUSY;

use super::{Listener, Socket};

impl Listener {
    pub fn bind_named_pipe(addr: impl AsRef<str>) -> IoResult<Self> {
        let listener = ServerOptions::new()
            .first_pipe_instance(true)
            .create(addr.as_ref())?;
        Ok(Self::from((listener, addr)))
    }
}

impl Socket {
    pub async fn connect_named_pipe(addr: impl AsRef<str>) -> IoResult<Self> {
        let client = loop {
            match ClientOptions::new().open(addr.as_ref()) {
                Ok(client) => break client,
                Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) => (),
                Err(e) => return Err(e),
            }
            sleep(Duration::from_millis(50)).await;
        };

        Ok(Self::from(client))
    }
}

impl<T: AsRef<str>> From<(NamedPipeServer, T)> for Listener {
    fn from((mut listener, addr): (NamedPipeServer, T)) -> Self {
        let addr = addr.as_ref().to_string();
        Self::new(stream! {
            loop {
                yield listener
                    .connect()
                    .await
                    .and_then(|_| ServerOptions::new().create(&addr))
                    .map(|l| replace(&mut listener, l));
            }
        })
    }
}

impl From<NamedPipeServer> for Socket {
    fn from(socket: NamedPipeServer) -> Self {
        Self::new(socket)
    }
}

impl From<NamedPipeClient> for Socket {
    fn from(socket: NamedPipeClient) -> Self {
        Self::new(socket)
    }
}
