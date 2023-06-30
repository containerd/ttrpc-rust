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

//! Sync server of ttrpc.
//!

#[cfg(unix)]
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};

use protobuf::{CodedInputStream, Message};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, sync_channel, Receiver, Sender, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::thread::JoinHandle;

use super::utils::{response_error_to_channel, response_to_channel};
use crate::context;
use crate::error::{get_status, Error, Result};
use crate::proto::{Code, MessageHeader, Request, Response, MESSAGE_TYPE_REQUEST};
use crate::sync::channel::{read_message, write_message};
use crate::sync::sys::{PipeConnection, PipeListener};
use crate::{MethodHandler, TtrpcContext};

// poll_queue will create WAIT_THREAD_COUNT_DEFAULT threads in begin.
// If wait thread count < WAIT_THREAD_COUNT_MIN, create number to WAIT_THREAD_COUNT_DEFAULT.
// If wait thread count > WAIT_THREAD_COUNT_MAX, wait thread will quit to WAIT_THREAD_COUNT_DEFAULT.
const DEFAULT_WAIT_THREAD_COUNT_DEFAULT: usize = 3;
const DEFAULT_WAIT_THREAD_COUNT_MIN: usize = 1;
const DEFAULT_WAIT_THREAD_COUNT_MAX: usize = 5;

type MessageSender = Sender<(MessageHeader, Vec<u8>)>;
type MessageReceiver = Receiver<(MessageHeader, Vec<u8>)>;
type WorkloadSender = crossbeam::channel::Sender<(MessageHeader, Result<Vec<u8>>)>;
type WorkloadReceiver = crossbeam::channel::Receiver<(MessageHeader, Result<Vec<u8>>)>;

/// A ttrpc Server (sync).
pub struct Server {
    listeners: Vec<Arc<PipeListener>>,
    listener_quit_flag: Arc<AtomicBool>,
    connections: Arc<Mutex<HashMap<i32, Connection>>>,
    methods: Arc<HashMap<String, Box<dyn MethodHandler + Send + Sync>>>,
    handler: Option<JoinHandle<()>>,
    reaper: Option<(Sender<i32>, JoinHandle<()>)>,
    thread_count_default: usize,
    thread_count_min: usize,
    thread_count_max: usize,
}

struct Connection {
    connection: Arc<PipeConnection>,
    quit: Arc<AtomicBool>,
    handler: Option<JoinHandle<()>>,
}

impl Connection {
    fn close(&self) {
        self.connection.close().unwrap_or(());
    }

    fn shutdown(&self) {
        self.quit.store(true, Ordering::SeqCst);

        // in case the connection had closed
        self.connection.shutdown().unwrap_or(());
    }
}

struct ThreadS<'a> {
    connection: &'a Arc<PipeConnection>,
    workload_rx: &'a WorkloadReceiver,
    wtc: &'a Arc<AtomicUsize>,
    quit: &'a Arc<AtomicBool>,
    methods: &'a Arc<HashMap<String, Box<dyn MethodHandler + Send + Sync>>>,
    res_tx: &'a MessageSender,
    control_tx: &'a SyncSender<()>,
    cancel_rx: &'a crossbeam::channel::Receiver<()>,
    default: usize,
    min: usize,
    max: usize,
}

#[allow(clippy::too_many_arguments)]
fn start_method_handler_thread(
    connection: Arc<PipeConnection>,
    workload_rx: WorkloadReceiver,
    wtc: Arc<AtomicUsize>,
    quit: Arc<AtomicBool>,
    methods: Arc<HashMap<String, Box<dyn MethodHandler + Send + Sync>>>,
    res_tx: MessageSender,
    control_tx: SyncSender<()>,
    cancel_rx: crossbeam::channel::Receiver<()>,
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

            let result = workload_rx.recv();

            if quit.load(Ordering::SeqCst) {
                // notify the connection dealing main thread to stop.
                control_tx
                    .send(())
                    .unwrap_or_else(|err| trace!("Failed to send {:?}", err));
                break;
            }

            let c = wtc.fetch_sub(1, Ordering::SeqCst) - 1;
            if c < min {
                trace!("notify client handler to create much more worker threads!");
                control_tx
                    .send(())
                    .unwrap_or_else(|err| trace!("Failed to send {:?}", err));
            }

            let mh;
            let buf;
            match result {
                Ok((x, Ok(y))) => {
                    mh = x;
                    buf = y;
                }
                Ok((mh, Err(e))) => {
                    if let Err(x) = response_error_to_channel(mh.stream_id, e, res_tx.clone()) {
                        debug!("response_error_to_channel get error {:?}", x);
                        quit_connection(quit, control_tx);
                        break;
                    }
                    continue;
                }
                Err(x) => match x {
                    crossbeam::channel::RecvError => {
                        trace!("workload_rx recv error");
                        quit_connection(quit, control_tx);
                        trace!("workload_rx recv error, send control_tx");
                        break;
                    }
                },
            }

            if mh.type_ != MESSAGE_TYPE_REQUEST {
                continue;
            }
            let mut s = CodedInputStream::from_bytes(&buf);
            let mut req = Request::new();
            if let Err(x) = req.merge_from(&mut s) {
                let status = get_status(Code::INVALID_ARGUMENT, x.to_string());
                let mut res = Response::new();
                res.set_status(status);
                if let Err(x) = response_to_channel(mh.stream_id, res, res_tx.clone()) {
                    debug!("response_to_channel get error {:?}", x);
                    quit_connection(quit, control_tx);
                    break;
                }
                continue;
            }
            trace!("Got Message request {:?}", req);

            let path = format!("/{}/{}", req.service, req.method);
            let method = if let Some(x) = methods.get(&path) {
                x
            } else {
                let status = get_status(Code::INVALID_ARGUMENT, format!("{path} does not exist"));
                let mut res = Response::new();
                res.set_status(status);
                if let Err(x) = response_to_channel(mh.stream_id, res, res_tx.clone()) {
                    info!("response_to_channel get error {:?}", x);
                    quit_connection(quit, control_tx);
                    break;
                }
                continue;
            };
            let ctx = TtrpcContext {
                fd: connection.id(),
                cancel_rx: cancel_rx.clone(),
                mh,
                res_tx: res_tx.clone(),
                metadata: context::from_pb(&req.metadata),
                timeout_nano: req.timeout_nano,
            };
            if let Err(x) = method.handler(ctx, req) {
                debug!("method handle {} get error {:?}", path, x);
                quit_connection(quit, control_tx);
                break;
            }
        }
    });
}

fn start_method_handler_threads(num: usize, ts: &ThreadS) {
    for _ in 0..num {
        if ts.quit.load(Ordering::SeqCst) {
            break;
        }
        start_method_handler_thread(
            ts.connection.clone(),
            ts.workload_rx.clone(),
            ts.wtc.clone(),
            ts.quit.clone(),
            ts.methods.clone(),
            ts.res_tx.clone(),
            ts.control_tx.clone(),
            ts.cancel_rx.clone(),
            ts.min,
            ts.max,
        );
    }
}

fn check_method_handler_threads(ts: &ThreadS) {
    let c = ts.wtc.load(Ordering::SeqCst);
    if c < ts.min {
        start_method_handler_threads(ts.default - c, ts);
    }
}

impl Default for Server {
    fn default() -> Self {
        Server {
            listeners: Vec::with_capacity(1),
            listener_quit_flag: Arc::new(AtomicBool::new(false)),
            connections: Arc::new(Mutex::new(HashMap::new())),
            methods: Arc::new(HashMap::new()),
            handler: None,
            reaper: None,
            thread_count_default: DEFAULT_WAIT_THREAD_COUNT_DEFAULT,
            thread_count_min: DEFAULT_WAIT_THREAD_COUNT_MIN,
            thread_count_max: DEFAULT_WAIT_THREAD_COUNT_MAX,
        }
    }
}

impl Server {
    pub fn new() -> Server {
        Server::default()
    }

    pub fn bind(mut self, sockaddr: &str) -> Result<Server> {
        if !self.listeners.is_empty() {
            return Err(Error::Others(
                "ttrpc-rust just support 1 sockaddr now".to_string(),
            ));
        }

        let listener = PipeListener::new(sockaddr)?;

        self.listeners.push(Arc::new(listener));
        Ok(self)
    }

    #[cfg(unix)]
    pub fn add_listener(mut self, fd: RawFd) -> Result<Server> {
        if !self.listeners.is_empty() {
            return Err(Error::Others(
                "ttrpc-rust just support 1 sockaddr now".to_string(),
            ));
        }

        let listener = PipeListener::new_from_fd(fd)?;

        self.listeners.push(Arc::new(listener));

        Ok(self)
    }

    pub fn register_service(
        mut self,
        methods: HashMap<String, Box<dyn MethodHandler + Send + Sync>>,
    ) -> Server {
        let mut_methods = Arc::get_mut(&mut self.methods).unwrap();
        mut_methods.extend(methods);
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

    pub fn start_listen(&mut self) -> Result<()> {
        let connections = self.connections.clone();

        if self.listeners.is_empty() {
            return Err(Error::Others("ttrpc-rust not bind".to_string()));
        }

        self.listener_quit_flag.store(false, Ordering::SeqCst);

        let listener = self.listeners[0].clone();
        let methods = self.methods.clone();
        let default = self.thread_count_default;
        let min = self.thread_count_min;
        let max = self.thread_count_max;
        let listener_quit_flag = self.listener_quit_flag.clone();

        let reaper_tx = match self.reaper.take() {
            None => {
                let reaper_connections = connections.clone();
                let (reaper_tx, reaper_rx) = channel();
                let reaper_handler = thread::Builder::new()
                    .name("reaper".into())
                    .spawn(move || {
                        for fd in reaper_rx.iter() {
                            reaper_connections
                                .lock()
                                .unwrap()
                                .remove(&fd)
                                .map(|mut cn| {
                                    cn.handler.take().map(|handler| {
                                        handler.join().unwrap();
                                        cn.close()
                                    })
                                });
                        }
                        info!("reaper thread exited");
                    })
                    .unwrap();
                self.reaper = Some((reaper_tx.clone(), reaper_handler));
                reaper_tx
            }
            Some(r) => {
                let reaper_tx = r.0.clone();
                self.reaper = Some(r);
                reaper_tx
            }
        };

        let handler = thread::Builder::new()
            .name("listener_loop".into())
            .spawn(move || {
                loop {
                    trace!("listening...");
                    let pipe_connection = match listener.accept(&listener_quit_flag) {
                        Ok(None) => {
                            continue;
                        }
                        Ok(Some(conn)) => Arc::new(conn),
                        Err(e) => {
                            error!("listener accept got {:?}", e);
                            break;
                        }
                    };

                    let methods = methods.clone();
                    let quit = Arc::new(AtomicBool::new(false));
                    let child_quit = quit.clone();
                    let reaper_tx_child = reaper_tx.clone();
                    let pipe_connection_child = pipe_connection.clone();

                    let handler = thread::Builder::new()
                        .name("client_handler".into())
                        .spawn(move || {
                            debug!("Got new client");
                            // Start response thread
                            let quit_res = child_quit.clone();
                            let pipe = pipe_connection_child.clone();
                            let (res_tx, res_rx): (MessageSender, MessageReceiver) = channel();
                            let handler = thread::spawn(move || {
                                for r in res_rx.iter() {
                                    trace!("response thread get {:?}", r);
                                    if let Err(e) = write_message(&pipe, r.0, r.1) {
                                        error!("write_message got {:?}", e);
                                        quit_res.store(true, Ordering::SeqCst);
                                        break;
                                    }
                                }

                                trace!("response thread quit");
                            });

                            let (control_tx, control_rx): (SyncSender<()>, Receiver<()>) =
                                sync_channel(0);

                            // start read message thread
                            let quit_reader = child_quit.clone();
                            let pipe_reader = pipe_connection_child.clone();
                            let (workload_tx, workload_rx): (WorkloadSender, WorkloadReceiver) =
                                crossbeam::channel::unbounded();
                            let (cancel_tx, cancel_rx) = crossbeam::channel::unbounded::<()>();
                            let control_tx_reader = control_tx.clone();
                            let reader = thread::spawn(move || {
                                while !quit_reader.load(Ordering::SeqCst) {
                                    let msg = read_message(&pipe_reader);
                                    match msg {
                                        Ok((x, y)) => {
                                            let res = workload_tx.send((x, y));
                                            match res {
                                                Ok(_) => {}
                                                Err(crossbeam::channel::SendError(e)) => {
                                                    error!("Send workload error {:?}", e);
                                                    quit_reader.store(true, Ordering::SeqCst);
                                                    control_tx_reader.send(()).unwrap_or_else(
                                                        |err| trace!("Failed to send {:?}", err),
                                                    );
                                                    break;
                                                }
                                            }
                                        }
                                        Err(x) => match x {
                                            Error::Socket(y) => {
                                                trace!("Socket error {}", y);
                                                drop(cancel_tx);
                                                quit_reader.store(true, Ordering::SeqCst);
                                                // the client connection would be closed and
                                                // the connection dealing main thread would
                                                // have exited.
                                                control_tx_reader.send(()).unwrap_or_else(|err| {
                                                    trace!("Failed to send {:?}", err)
                                                });
                                                trace!("Socket error send control_tx");
                                                break;
                                            }
                                            _ => {
                                                trace!("Other error {:?}", x);
                                                continue;
                                            }
                                        },
                                    }
                                }

                                trace!("read message thread quit");
                            });

                            let pipe = pipe_connection_child.clone();
                            let ts = ThreadS {
                                connection: &pipe,
                                workload_rx: &workload_rx,
                                wtc: &Arc::new(AtomicUsize::new(0)),
                                methods: &methods,
                                res_tx: &res_tx,
                                control_tx: &control_tx,
                                cancel_rx: &cancel_rx,
                                quit: &child_quit,
                                default,
                                min,
                                max,
                            };
                            start_method_handler_threads(ts.default, &ts);

                            while !child_quit.load(Ordering::SeqCst) {
                                check_method_handler_threads(&ts);
                                if control_rx.recv().is_err() {
                                    break;
                                }
                            }
                            // drop the control_rx, thus all of the method handler threads would
                            // terminated.
                            drop(control_rx);
                            // drop the res_tx, thus the res_rx would get terminated notification.
                            drop(res_tx);
                            drop(workload_rx);
                            handler.join().unwrap_or(());
                            reader.join().unwrap_or(());
                            // client_handler should not close fd before exit
                            // , which prevent fd reuse issue.
                            reaper_tx_child.send(pipe.id()).unwrap();

                            debug!("client thread quit");
                        })
                        .unwrap();

                    let mut cns = connections.lock().unwrap();

                    let id = pipe_connection.id();
                    cns.insert(
                        id,
                        Connection {
                            connection: pipe_connection,
                            handler: Some(handler),
                            quit: quit.clone(),
                        },
                    );
                } // end loop

                // notify reaper thread to exit.
                drop(reaper_tx);
                info!("ttrpc server listener stopped");
            })
            .unwrap();

        self.handler = Some(handler);
        info!("server listen started");
        Ok(())
    }

    pub fn start(&mut self) -> Result<()> {
        if self.thread_count_default >= self.thread_count_max {
            return Err(Error::Others(
                "thread_count_default should smaller than thread_count_max".to_string(),
            ));
        }
        if self.thread_count_default <= self.thread_count_min {
            return Err(Error::Others(
                "thread_count_default should bigger than thread_count_min".to_string(),
            ));
        }
        self.start_listen()?;
        info!("server started");
        Ok(())
    }

    pub fn stop_listen(mut self) -> Self {
        self.listener_quit_flag.store(true, Ordering::SeqCst);

        self.listeners[0]
            .close()
            .unwrap_or_else(|e| warn!("failed to close connection with error: {}", e));

        info!("close monitor");
        if let Some(handler) = self.handler.take() {
            handler.join().unwrap();
        }
        info!("listener thread stopped");
        self
    }

    pub fn disconnect(mut self) {
        info!("begin to shutdown connection");
        let connections = self.connections.lock().unwrap();

        for (_fd, c) in connections.iter() {
            c.shutdown();
        }
        // release connections's lock, since the following handler.join()
        // would wait on the other thread's exit in which would take the lock.
        drop(connections);
        info!("connections closed");

        if let Some(r) = self.reaper.take() {
            drop(r.0);
            r.1.join().unwrap();
        }
        info!("reaper thread stopped");
    }

    pub fn shutdown(self) {
        self.stop_listen().disconnect();
    }
}

#[cfg(unix)]
impl FromRawFd for Server {
    unsafe fn from_raw_fd(fd: RawFd) -> Self {
        Self::default().add_listener(fd).unwrap()
    }
}

#[cfg(unix)]
impl AsRawFd for Server {
    fn as_raw_fd(&self) -> RawFd {
        self.listeners[0].as_raw_fd()
    }
}

fn quit_connection(quit: Arc<AtomicBool>, control_tx: SyncSender<()>) {
    quit.store(true, Ordering::SeqCst);
    // the client connection would be closed and
    // the connection dealing main thread would
    // have exited.
    control_tx
        .send(())
        .unwrap_or_else(|err| debug!("Failed to send {:?}", err));
}
