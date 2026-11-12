//! Ownership of a client started by this adapter; attached clients never enter this type.
use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

use eska_build_runtime::runtime::{DebugLaunch, RuntimeError};

pub(super) struct Client {
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<Result<(), RuntimeError>>>,
}

impl Client {
    pub(super) fn launch(launch: DebugLaunch) -> io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let cancel = Arc::clone(&stop);
        let worker = thread::Builder::new().spawn(move || launch.run(&cancel))?;
        Ok(Self {
            stop,
            worker: Some(worker),
        })
    }

    pub(super) fn completed(&mut self) -> Option<bool> {
        if self.worker.as_ref()?.is_finished() {
            Some(
                self.worker
                    .take()?
                    .join()
                    .is_ok_and(|result| result.is_ok()),
            )
        } else {
            None
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_failure_is_reported_once_and_is_not_a_successful_exit() {
        let mut client = Client {
            stop: Arc::new(AtomicBool::new(false)),
            worker: Some(thread::spawn(|| Err(RuntimeError::ClientFailed))),
        };
        while !client.worker.as_ref().unwrap().is_finished() {
            thread::yield_now();
        }
        assert_eq!(client.completed(), Some(false));
        assert_eq!(client.completed(), None);
    }

    #[test]
    fn dropping_an_owned_client_requests_cancellation_and_reaps_it() {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let (send, receive) = std::sync::mpsc::channel();
        let worker = thread::spawn(move || {
            while !flag.load(Ordering::Acquire) {
                thread::yield_now();
            }
            send.send(()).unwrap();
            Ok(())
        });
        drop(Client {
            stop,
            worker: Some(worker),
        });
        assert!(receive.try_recv().is_ok());
    }
}
