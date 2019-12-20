// Copyright (c) 2019 Ant Financial
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use nix::sys::socket::*;
use nix::unistd::close;
use protobuf::{CodedInputStream, CodedOutputStream, Message};
use std::collections::HashMap;
use std::fmt;
use std::fmt::{Debug, Formatter};
use std::net::{IpAddr, SocketAddr};
use std::os::unix::io::RawFd;
use std::process;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, sync_channel, Receiver, Sender, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::channel::{
    message_header, read_message, write_message, MESSAGE_TYPE_REQUEST, MESSAGE_TYPE_RESPONSE,
};
use crate::error::{get_Status, Error, Result};
use crate::ttrpc::{Code, Request, Response, Status};

// poll_queue will create WAIT_THREAD_COUNT_DEFAULT threads in begin.
// If wait thread count < WAIT_THREAD_COUNT_MIN, create number to WAIT_THREAD_COUNT_DEFAULT.
// If wait thread count > WAIT_THREAD_COUNT_MAX, wait thread will quit to WAIT_THREAD_COUNT_DEFAULT.
const DEFAULT_WAIT_THREAD_COUNT_DEFAULT: usize = 3;
const DEFAULT_WAIT_THREAD_COUNT_MIN: usize = 1;
const DEFAULT_WAIT_THREAD_COUNT_MAX: usize = 5;

pub struct Server {
    listeners: Vec<RawFd>,
    methods: HashMap<String, Box<MethodHandler + Send + Sync>>,
    thread_count_default: usize,
    thread_count_min: usize,
    thread_count_max: usize,
}

struct ThreadS<'a> {
    fd: RawFd,
    fdlock: &'a Arc<Mutex<()>>,
    wtc: &'a Arc<AtomicUsize>,
    quit: &'a Arc<AtomicBool>,
    methods: &'a Arc<HashMap<String, Box<MethodHandler + Send + Sync>>>,
    res_tx: &'a Sender<(message_header, Vec<u8>)>,
    control_tx: &'a SyncSender<()>,
    default: usize,
    min: usize,
    max: usize,
}

fn start_method_handler_thread(
    fd: RawFd,
    fdlock: Arc<Mutex<()>>,
    wtc: Arc<AtomicUsize>,
    quit: Arc<AtomicBool>,
    methods: Arc<HashMap<String, Box<MethodHandler + Send + Sync>>>,
    res_tx: Sender<(message_header, Vec<u8>)>,
    control_tx: SyncSender<()>,
    min: usize,
    max: usize,
) {
    thread::spawn(move || {
        while !quit.load(Ordering::SeqCst) {
            let c = wtc.fetch_add(1, Ordering::SeqCst) + 1;
            if c > max {
                wtc.fetch_sub(1, Ordering::SeqCst);
                break;
            }

            let result;
            {
                let guard = fdlock.lock().unwrap();
                if quit.load(Ordering::SeqCst) {
                    break;
                }
                result = read_message(fd);
            }

            if quit.load(Ordering::SeqCst) {
                break;
            }

            let c = wtc.fetch_sub(1, Ordering::SeqCst) - 1;
            if c < min {
                control_tx.try_send(());
            }

            let mh;
            let buf;
            match result {
                Ok((x, y)) => {
                    mh = x;
                    buf = y;
                }
                Err(x) => match x {
                    Error::Socket(y) => {
                        trace!("Socket error {}", y);
                        quit.store(true, Ordering::SeqCst);
                        control_tx.try_send(());
                        break;
                    }
                    _ => {
                        trace!("Others error {:?}", x);
                        continue;
                    }
                },
            }

            if mh.Type != MESSAGE_TYPE_REQUEST {
                continue;
            }
            let mut s = CodedInputStream::from_bytes(&buf);
            let mut req = Request::new();
            if let Err(x) = req.merge_from(&mut s) {
                let status = get_Status(Code::INVALID_ARGUMENT, x.to_string());
                let mut res = Response::new();
                res.set_status(status);
                if let Err(x) = response_to_channel(mh.StreamID, res, res_tx.clone()) {
                    debug!("response_to_channel get error {:?}", x);
                    quit.store(true, Ordering::SeqCst);
                    control_tx.try_send(());
                    break;
                }
                continue;
            }
            trace!("Got Message request {:?}", req);

            let path = format!("/{}/{}", req.service, req.method);
            let method;
            if let Some(x) = methods.get(&path) {
                method = x;
            } else {
                let status = get_Status(Code::INVALID_ARGUMENT, format!("{} does not exist", path));
                let mut res = Response::new();
                res.set_status(status);
                if let Err(x) = response_to_channel(mh.StreamID, res, res_tx.clone()) {
                    debug!("response_to_channel get error {:?}", x);
                    quit.store(true, Ordering::SeqCst);
                    control_tx.try_send(());
                    break;
                }
                continue;
            }
            let ctx = TtrpcContext {
                fd: fd,
                mh: mh,
                res_tx: res_tx.clone(),
            };
            if let Err(x) = method.handler(ctx, req) {
                debug!("method handle {} get error {:?}", path, x);
                quit.store(true, Ordering::SeqCst);
                control_tx.try_send(());
                break;
            }
        }
    });
}

fn start_method_handler_threads(num: usize, ts: &ThreadS) {
    for _ in 0..num {
        if (ts.quit.load(Ordering::SeqCst)) {
            break;
        }
        start_method_handler_thread(
            ts.fd,
            ts.fdlock.clone(),
            ts.wtc.clone(),
            ts.quit.clone(),
            ts.methods.clone(),
            ts.res_tx.clone(),
            ts.control_tx.clone(),
            ts.min,
            ts.max,
        );
    }
}

fn check_method_handler_threads(ts: &ThreadS) {
    let c = ts.wtc.load(Ordering::SeqCst);
    if c < ts.min {
        start_method_handler_threads(ts.default - c, &ts);
    }
}

impl Server {
    pub fn new() -> Server {
        Server {
            listeners: Vec::with_capacity(1),
            methods: HashMap::new(),
            thread_count_default: DEFAULT_WAIT_THREAD_COUNT_DEFAULT,
            thread_count_min: DEFAULT_WAIT_THREAD_COUNT_MIN,
            thread_count_max: DEFAULT_WAIT_THREAD_COUNT_MAX,
        }
    }

    pub fn bind(mut self, host: &str) -> Result<Server> {
        if self.listeners.len() > 0 {
            return Err(Error::Others(format!("ttrpc-rust just support 1 host now")));
        }

        let hostv: Vec<&str> = host.trim().split("://").collect();
        if hostv.len() != 2 {
            return Err(Error::Others(format!("Host {} is not right", host)));
        }
        let scheme = hostv[0].to_lowercase();

        let sockaddr: SockAddr;
        let fd: RawFd;

        match scheme.as_str() {
            "unix" => {
                fd = socket(
                    AddressFamily::Unix,
                    SockType::Stream,
                    SockFlag::empty(),
                    None,
                )
                .map_err(|e| Error::Socket(e.to_string()))?;
                let sockaddr_h = hostv[1].to_owned() + &"\x00".to_string();
                let sockaddr_u =
                    UnixAddr::new_abstract(sockaddr_h.as_bytes()).map_err(err_to_Others!(e, ""))?;
                sockaddr = SockAddr::Unix(sockaddr_u);
            }

            "vsock" => {
                let host_port_v: Vec<&str> = hostv[1].split(":").collect();
                if host_port_v.len() != 2 {
                    return Err(Error::Others(format!(
                        "Host {} is not right for vsock",
                        host
                    )));
                }
                let cid = libc::VMADDR_CID_ANY;
                let port: u32 =
                    FromStr::from_str(host_port_v[1]).expect("the vsock port is not an number");
                fd = socket(
                    AddressFamily::Vsock,
                    SockType::Stream,
                    SockFlag::empty(),
                    None,
                )
                .map_err(|e| Error::Socket(e.to_string()))?;
                sockaddr = SockAddr::new_vsock(cid, port);
            }
            _ => return Err(Error::Others(format!("Scheme {} is not supported", scheme))),
        };

        bind(fd, &sockaddr).map_err(err_to_Others!(e, ""))?;
        self.listeners.push(fd);

        Ok(self)
    }

    pub fn register_service(
        mut self,
        methods: HashMap<String, Box<MethodHandler + Send + Sync>>,
    ) -> Server {
        self.methods.extend(methods);
        self
    }

    pub fn set_thread_count_default(mut self, count: usize) -> Server {
        self.thread_count_default = count;
        self
    }

    pub fn set_thread_count_min(mut self, count: usize) -> Server {
        self.thread_count_min = count;
        self
    }

    pub fn set_thread_count_max(mut self, count: usize) -> Server {
        self.thread_count_max = count;
        self
    }

    pub fn start(self) -> Result<()> {
        if self.thread_count_default >= self.thread_count_max {
            return Err(Error::Others(format!(
                "thread_count_default should smaller than thread_count_max"
            )));
        }
        if self.thread_count_default <= self.thread_count_min {
            return Err(Error::Others(format!(
                "thread_count_default should biger than thread_count_min"
            )));
        }

        if self.listeners.len() <= 0 {
            return Err(Error::Others(format!("ttrpc-rust not bind")));
        }
        listen(self.listeners[0], 10).map_err(|e| Error::Socket(e.to_string()))?;
        let methods = Arc::new(self.methods);
        let default = self.thread_count_default;
        let min = self.thread_count_min;
        let max = self.thread_count_max;
        loop {
            let fd = accept(self.listeners[0]).map_err(|e| Error::Socket(e.to_string()))?;
            let methods = methods.clone();
            let quit = Arc::new(AtomicBool::new(false));
            thread::spawn(move || {
                trace!("Got new client");

                // Start response thread
                let quit_res = quit.clone();
                let (res_tx, res_rx): (
                    Sender<(message_header, Vec<u8>)>,
                    Receiver<(message_header, Vec<u8>)>,
                ) = channel();
                thread::spawn(move || {
                    for r in res_rx.iter() {
                        if (quit_res.load(Ordering::SeqCst)) {
                            break;
                        }
                        trace!("response thread get {:?}", r);
                        if let Err(e) = write_message(fd, r.0, r.1) {
                            trace!("write_message got {:?}", e);
                            quit_res.store(true, Ordering::SeqCst);
                            break;
                        }
                    }
                    trace!("response thread quit");
                });

                let (control_tx, control_rx): (SyncSender<()>, Receiver<()>) = sync_channel(0);
                let ts = ThreadS {
                    fd: fd,
                    fdlock: &Arc::new(Mutex::new(())),
                    wtc: &Arc::new(AtomicUsize::new(0)),
                    methods: &methods,
                    res_tx: &res_tx,
                    control_tx: &control_tx,
                    quit: &quit,
                    default: default,
                    min: min,
                    max: max,
                };
                start_method_handler_threads(ts.default, &ts);

                while !quit.load(Ordering::SeqCst) {
                    check_method_handler_threads(&ts);
                    if let Err(_) = control_rx.recv() {
                        break;
                    }
                }

                close(fd);
                trace!("client thread quit");
            });
        }

        Ok(())
    }
}

pub struct TtrpcContext {
    pub fd: RawFd,
    pub mh: message_header,
    pub res_tx: Sender<(message_header, Vec<u8>)>,
}

pub trait MethodHandler {
    fn handler(&self, ctx: TtrpcContext, req: Request) -> Result<()>;
}

pub fn response_to_channel(
    StreamID: u32,
    res: Response,
    tx: Sender<(message_header, Vec<u8>)>,
) -> Result<()> {
    let mut buf = Vec::with_capacity(res.compute_size() as usize);
    let mut s = CodedOutputStream::vec(&mut buf);
    res.write_to(&mut s).map_err(err_to_Others!(e, ""))?;
    s.flush().map_err(err_to_Others!(e, ""))?;

    let mh = message_header {
        Length: buf.len() as u32,
        StreamID: StreamID,
        Type: MESSAGE_TYPE_RESPONSE,
        Flags: 0,
    };
    tx.send((mh, buf)).map_err(err_to_Others!(e, ""))?;

    Ok(())
}

#[macro_export]
macro_rules! request_handler {
    ($class: ident, $ctx: ident, $req: ident, $server: ident, $req_type: ident, $req_fn: ident) => {
        let mut s = CodedInputStream::from_bytes(&$req.payload);
        let mut req = super::$server::$req_type::new();
        req.merge_from(&mut s)
            .map_err(::ttrpc::Err_to_Others!(e, ""))?;

        let mut res = ::ttrpc::Response::new();
        match $class.service.$req_fn(&$ctx, req) {
            Ok(rep) => {
                res.set_status(::ttrpc::get_Status(::ttrpc::Code::OK, "".to_string()));
                res.payload.reserve(rep.compute_size() as usize);
                let mut s = CodedOutputStream::vec(&mut res.payload);
                rep.write_to(&mut s)
                    .map_err(::ttrpc::Err_to_Others!(e, ""))?;
                s.flush().map_err(::ttrpc::Err_to_Others!(e, ""))?;
            }
            Err(x) => match x {
                ::ttrpc::Error::RpcStatus(s) => {
                    res.set_status(s);
                }
                _ => {
                    res.set_status(::ttrpc::get_Status(
                        ::ttrpc::Code::UNKNOWN,
                        format!("{:?}", x),
                    ));
                }
            },
        }
        ::ttrpc::response_to_channel($ctx.mh.StreamID, res, $ctx.res_tx)?
    };
}
