// Copyright 2022 Alibaba Cloud. All rights reserved.
//
// SPDX-License-Identifier: Apache-2.0
//

//! Cooperative shutdown notification.
//!
//! A [`Notifier`](crate::asynchronous::shutdown::Notifier) owns the shutdown state and one or more
//! cloneable [`Waiter`](crate::asynchronous::shutdown::Waiter) values observe it. Dropping the
//! notifier also initiates shutdown. This module is used by the async server and can also
//! coordinate application tasks.
//!
//! # Examples
//!
//! ```
//! # async fn run() {
//! let (notifier, waiter) = ttrpc::r#async::shutdown::new();
//!
//! notifier.shutdown();
//! waiter.wait_shutdown().await;
//! assert!(waiter.is_shutdown());
//! # }
//! ```

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use tokio::sync::Notify;
use tokio::time::{error::Elapsed, timeout, Duration};

#[derive(Debug)]
struct Shared {
    shutdown: AtomicBool,
    notify_shutdown: Notify,

    waiters: AtomicUsize,
    notify_exit: Notify,
}

impl Shared {
    fn is_shutdown(&self) -> bool {
        self.shutdown.load(Ordering::Relaxed)
    }
}

/// A cloneable handle that waits for a shutdown notification.
///
/// Each clone counts as an active waiter until it is dropped. [`Notifier::wait_all_exit`] completes
/// after every waiter has been dropped.
#[derive(Debug)]
pub struct Waiter {
    shared: Arc<Shared>,
}

/// Initiates shutdown and tracks active [`Waiter`] handles.
///
/// `Notifier` is deliberately not [`Clone`]. Wrap it in [`std::sync::Arc`] when multiple tasks need
/// to initiate or observe shutdown. Dropping it calls [`Notifier::shutdown`].
#[derive(Debug)]
pub struct Notifier {
    shared: Arc<Shared>,
    wait_time: Option<Duration>,
}

/// Creates a notifier and its first waiter without an exit timeout.
pub fn new() -> (Notifier, Waiter) {
    _with_timeout(None)
}

/// Creates a notifier and its first waiter with an exit timeout.
///
/// `wait_time` limits [`Notifier::wait_all_exit`]; it does not delay or time out delivery of the
/// shutdown notification itself.
pub fn with_timeout(wait_time: Duration) -> (Notifier, Waiter) {
    _with_timeout(Some(wait_time))
}

fn _with_timeout(wait_time: Option<Duration>) -> (Notifier, Waiter) {
    let shared = Arc::new(Shared {
        shutdown: AtomicBool::new(false),
        waiters: AtomicUsize::new(1),
        notify_shutdown: Notify::new(),
        notify_exit: Notify::new(),
    });

    let notifier = Notifier {
        shared: shared.clone(),
        wait_time,
    };

    let waiter = Waiter { shared };

    (notifier, waiter)
}

impl Waiter {
    /// Returns `true` after [`Notifier::shutdown`] has been called or the notifier was dropped.
    pub fn is_shutdown(&self) -> bool {
        self.shared.is_shutdown()
    }

    /// Waits until shutdown has been requested.
    ///
    /// If shutdown was already requested, this method returns immediately.
    pub async fn wait_shutdown(&self) {
        while !self.is_shutdown() {
            let shutdown = self.shared.notify_shutdown.notified();
            if self.is_shutdown() {
                return;
            }
            shutdown.await;
        }
    }

    fn from_shared(shared: Arc<Shared>) -> Self {
        shared.waiters.fetch_add(1, Ordering::Relaxed);
        Self { shared }
    }
}

impl Clone for Waiter {
    fn clone(&self) -> Self {
        Self::from_shared(self.shared.clone())
    }
}

impl Drop for Waiter {
    fn drop(&mut self) {
        if 1 == self.shared.waiters.fetch_sub(1, Ordering::Relaxed) {
            self.shared.notify_exit.notify_waiters();
        }
    }
}

impl Notifier {
    /// Returns `true` after shutdown has been requested.
    pub fn is_shutdown(&self) -> bool {
        self.shared.is_shutdown()
    }

    /// Requests shutdown and wakes all current waiters.
    ///
    /// Calling this method more than once has no additional effect.
    pub fn shutdown(&self) {
        let is_shutdown = self.shared.shutdown.swap(true, Ordering::Relaxed);
        if !is_shutdown {
            self.shared.notify_shutdown.notify_waiters();
        }
    }

    /// Returns the number of live [`Waiter`] handles.
    pub fn waiters(&self) -> usize {
        self.shared.waiters.load(Ordering::Relaxed)
    }

    /// Creates another waiter subscribed to this notifier.
    pub fn subscribe(&self) -> Waiter {
        Waiter::from_shared(self.shared.clone())
    }

    /// Waits for all [`Waiter`] handles to be dropped.
    ///
    /// # Errors
    ///
    /// Returns [`tokio::time::error::Elapsed`] if the timeout configured by [`with_timeout`]
    /// expires first. A pair created by [`new`] waits without a timeout.
    pub async fn wait_all_exit(&self) -> Result<(), Elapsed> {
        //debug_assert!(self.shared.is_shutdown());
        if let Some(tm) = self.wait_time {
            timeout(tm, self.wait()).await
        } else {
            self.wait().await;
            Ok(())
        }
    }

    async fn wait(&self) {
        while self.waiters() > 0 {
            let notified = self.shared.notify_exit.notified();
            if self.waiters() == 0 {
                return;
            }
            notified.await;
            // Some waiters could have been created in the meantime 
            // by calling `subscribe`, loop again
        }
    }
}

impl Drop for Notifier {
    fn drop(&mut self) {
        self.shutdown()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[tokio::test]
    async fn it_work() {
        let (notifier, waiter) = new();

        let task = tokio::spawn(async move {
            waiter.wait_shutdown().await;
        });

        assert_eq!(notifier.waiters(), 1);
        notifier.shutdown();
        task.await.unwrap();
        assert_eq!(notifier.waiters(), 0);
    }

    #[tokio::test]
    async fn notifier_drop() {
        let (notifier, waiter) = new();
        assert_eq!(notifier.waiters(), 1);
        assert!(!waiter.is_shutdown());
        drop(notifier);
        assert!(waiter.is_shutdown());
        assert_eq!(waiter.shared.waiters.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn waiter_clone() {
        let (notifier, waiter1) = new();
        assert_eq!(notifier.waiters(), 1);

        let waiter2 = waiter1.clone();
        assert_eq!(notifier.waiters(), 2);

        let waiter3 = notifier.subscribe();
        assert_eq!(notifier.waiters(), 3);

        drop(waiter2);
        assert_eq!(notifier.waiters(), 2);

        let task = tokio::spawn(async move {
            waiter3.wait_shutdown().await;
            assert!(waiter3.is_shutdown());
        });

        assert!(!waiter1.is_shutdown());
        notifier.shutdown();
        assert!(waiter1.is_shutdown());

        task.await.unwrap();

        assert_eq!(notifier.waiters(), 1);
    }

    #[tokio::test]
    async fn concurrency_notifier_shutdown() {
        let (notifier, waiter) = new();
        let arc_notifier = Arc::new(notifier);
        let notifier1 = arc_notifier.clone();
        let notifier2 = notifier1.clone();

        let task1 = tokio::spawn(async move {
            assert_eq!(notifier1.waiters(), 1);

            let waiter = notifier1.subscribe();
            assert_eq!(notifier1.waiters(), 2);

            notifier1.shutdown();
            waiter.wait_shutdown().await;
        });

        let task2 = tokio::spawn(async move {
            assert_eq!(notifier2.waiters(), 1);
            notifier2.shutdown();
        });
        waiter.wait_shutdown().await;
        assert!(arc_notifier.is_shutdown());
        task1.await.unwrap();
        task2.await.unwrap();
    }

    #[tokio::test]
    async fn concurrency_notifier_wait() {
        let (notifier, waiter) = new();
        let arc_notifier = Arc::new(notifier);
        let notifier1 = arc_notifier.clone();
        let notifier2 = notifier1.clone();

        let task1 = tokio::spawn(async move {
            notifier1.shutdown();
            notifier1.wait_all_exit().await.unwrap();
        });

        let task2 = tokio::spawn(async move {
            notifier2.shutdown();
            notifier2.wait_all_exit().await.unwrap();
        });

        waiter.wait_shutdown().await;
        drop(waiter);
        task1.await.unwrap();
        task2.await.unwrap();
    }

    #[tokio::test]
    async fn wait_all_exit() {
        let (notifier, waiter) = new();
        let mut tasks = Vec::with_capacity(100);
        for i in 0..100 {
            assert_eq!(notifier.waiters(), 1 + i);
            let waiter1 = waiter.clone();
            tasks.push(tokio::spawn(async move {
                waiter1.wait_shutdown().await;
            }));
        }
        drop(waiter);
        assert_eq!(notifier.waiters(), 100);
        notifier.shutdown();
        notifier.wait_all_exit().await.unwrap();
        for t in tasks {
            t.await.unwrap();
        }
    }

    #[tokio::test]
    async fn wait_timeout() {
        let (notifier, waiter) = with_timeout(Duration::from_millis(100));
        let task = tokio::spawn(async move {
            waiter.wait_shutdown().await;
            tokio::time::sleep(Duration::from_millis(200)).await;
        });
        notifier.shutdown();
        // Elapsed
        assert!(notifier.wait_all_exit().await.is_err());
        task.await.unwrap();
    }
}
