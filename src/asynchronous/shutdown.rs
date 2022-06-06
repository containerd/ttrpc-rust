// Copyright 2022 Alibaba Cloud. All rights reserved.
//
// SPDX-License-Identifier: Apache-2.0
//

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

/// Wait for the shutdown notification.
#[derive(Debug)]
pub struct Waiter {
    shared: Arc<Shared>,
}

/// Used to Notify all [`Waiter`s](Waiter) shutdown.
///
/// No `Clone` is provided. If you want multiple instances, you can use Arc<Notifier>.
/// Notifier will automatically call shutdown when dropping.
#[derive(Debug)]
pub struct Notifier {
    shared: Arc<Shared>,
    wait_time: Option<Duration>,
}

/// Create a new shutdown pair([`Notifier`], [`Waiter`]) without timeout.
///
/// The [`Notifier`]
pub fn new() -> (Notifier, Waiter) {
    _with_timeout(None)
}

/// Create a new shutdown pair with the specified [`Duration`].
///
/// The [`Duration`] is used to specify the timeout of the [`Notifier::wait_all_exit()`].
///
/// [`Duration`]: tokio::time::Duration
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
    /// Return `true` if the [`Notifier::shutdown()`] has been called.
    ///
    /// [`Notifier::shutdown()`]: Notifier::shutdown()
    pub fn is_shutdown(&self) -> bool {
        self.shared.is_shutdown()
    }

    /// Waiting for the [`Notifier::shutdown()`] to be called.
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
    /// Return `true` if the [`Notifier::shutdown()`] has been called.
    ///
    /// [`Notifier::shutdown()`]: Notifier::shutdown()
    pub fn is_shutdown(&self) -> bool {
        self.shared.is_shutdown()
    }

    /// Notify all [`Waiter`s](Waiter) shutdown.
    ///
    /// It will cause all calls blocking at `Waiter::wait_shutdown().await` to return.
    pub fn shutdown(&self) {
        let is_shutdown = self.shared.shutdown.swap(true, Ordering::Relaxed);
        if !is_shutdown {
            self.shared.notify_shutdown.notify_waiters();
        }
    }

    /// Return the num of all [`Waiter`]s.
    pub fn waiters(&self) -> usize {
        self.shared.waiters.load(Ordering::Relaxed)
    }

    /// Create a new [`Waiter`].
    pub fn subscribe(&self) -> Waiter {
        Waiter::from_shared(self.shared.clone())
    }

    /// Wait for all [`Waiter`]s to drop.
    pub async fn wait_all_exit(&self) -> Result<(), Elapsed> {
        //debug_assert!(self.shared.is_shutdown());
        if self.waiters() == 0 {
            return Ok(());
        }
        let wait = self.wait();
        if self.waiters() == 0 {
            return Ok(());
        }
        wait.await
    }

    async fn wait(&self) -> Result<(), Elapsed> {
        if let Some(tm) = self.wait_time {
            timeout(tm, self.shared.notify_exit.notified()).await
        } else {
            self.shared.notify_exit.notified().await;
            Ok(())
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
        assert!(matches!(notifier.wait_all_exit().await, Err(_)));
        task.await.unwrap();
    }
}
