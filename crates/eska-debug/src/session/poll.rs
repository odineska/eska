//! Successful poll replies, even idle ones, break a run of failures.
use super::{Input, Server};
use crate::http::Cancellation;
use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

const POLL_MIN: Duration = Duration::from_millis(30);
const POLL_MAX: Duration = Duration::from_millis(240);
const FAILURE_LIMIT: Duration = Duration::from_secs(5);

pub(super) struct Poll {
    stop: Arc<Cancellation>,
    failed: Arc<AtomicBool>,
    worker: thread::JoinHandle<()>,
}

impl Poll {
    pub(super) fn start(server: Arc<Server>, send: mpsc::SyncSender<Input>) -> io::Result<Self> {
        let stop = Arc::<Cancellation>::default();
        let failed = Arc::new(AtomicBool::new(false));
        let (cancel, overflow) = (Arc::clone(&stop), Arc::clone(&failed));
        let worker = thread::Builder::new().spawn(move || {
            let mut retry = Retry::new();
            while !cancel.is_cancelled() {
                let result = server.poll(Some(&cancel));
                if cancel.is_cancelled() {
                    break;
                }
                let active = result.as_ref().is_ok_and(|events| !events.is_empty());
                let delay = retry.observe(result.is_ok(), active, Instant::now());
                let message = match result {
                    Ok(events) if !events.is_empty() => Some(Ok(events)),
                    Err(error) if delay.is_none() => Some(Err(error)),
                    _ => None,
                };
                if let Some(message) = message
                    && send.try_send(Input::Events(message)).is_err()
                {
                    // Stops and calculations cannot be dropped. Idle replies and temporary
                    // failures stay here so they cannot exhaust the bounded session queue.
                    overflow.store(true, Ordering::Release);
                    break;
                }
                let Some(delay) = delay else { break };
                thread::park_timeout(delay);
            }
        })?;
        Ok(Self {
            stop,
            failed,
            worker,
        })
    }

    pub(super) fn failed(&self) -> bool {
        self.failed.load(Ordering::Acquire)
    }

    pub(super) fn stop(self) {
        self.stop.cancel();
        self.worker.thread().unpark();
        let _ = self.worker.join();
    }
}

struct Retry {
    failed_since: Option<Instant>,
    interval: Duration,
}

impl Retry {
    const fn new() -> Self {
        Self {
            failed_since: None,
            interval: POLL_MIN,
        }
    }

    fn observe(&mut self, healthy: bool, active: bool, now: Instant) -> Option<Duration> {
        if healthy {
            self.failed_since = None;
        } else if now.duration_since(*self.failed_since.get_or_insert(now)) >= FAILURE_LIMIT {
            return None;
        }
        self.interval = if active {
            POLL_MIN
        } else {
            (self.interval * 2).min(POLL_MAX)
        };
        Some(self.interval)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn successful_idle_reply_resets_the_failure_deadline() {
        let mut retry = Retry::new();
        let now = Instant::now();
        assert!(retry.observe(false, false, now).is_some());
        assert!(
            retry
                .observe(true, false, now + Duration::from_secs(4))
                .is_some()
        );
        assert!(
            retry
                .observe(false, false, now + Duration::from_secs(10))
                .is_some()
        );
        assert!(
            retry
                .observe(false, false, now + Duration::from_secs(14))
                .is_some()
        );
        assert!(
            retry
                .observe(false, false, now + Duration::from_secs(15))
                .is_none()
        );
    }

    #[test]
    fn idle_and_failed_polls_back_off_and_events_restore_responsiveness() {
        let mut retry = Retry::new();
        let now = Instant::now();
        assert_eq!(
            retry.observe(true, false, now),
            Some(Duration::from_millis(60))
        );
        assert_eq!(
            retry.observe(false, false, now),
            Some(Duration::from_millis(120))
        );
        assert_eq!(retry.observe(false, false, now), Some(POLL_MAX));
        assert_eq!(retry.observe(true, true, now), Some(POLL_MIN));
    }
}
