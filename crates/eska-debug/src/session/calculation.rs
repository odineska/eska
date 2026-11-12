use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::Instant,
};

use crate::{dbgs::CalculationQuery, http::Cancellation};

use super::{Input, MAX_PENDING, Server};

pub(super) struct Job {
    pub(super) query: CalculationQuery,
    pub(super) deadline: Instant,
    pub(super) cancellation: Arc<Cancellation>,
}

pub(super) struct Worker {
    jobs: mpsc::SyncSender<Job>,
    failed: Arc<AtomicBool>,
    thread: thread::JoinHandle<()>,
}

impl Worker {
    pub(super) fn start(server: Arc<Server>, input: mpsc::SyncSender<Input>) -> io::Result<Self> {
        let (jobs, receive) = mpsc::sync_channel::<Job>(MAX_PENDING);
        let failed = Arc::new(AtomicBool::new(false));
        let overflow = Arc::clone(&failed);
        let thread = thread::Builder::new().spawn(move || {
            while let Ok(job) = receive.recv() {
                if job.cancellation.is_cancelled() {
                    continue;
                }
                let result = server.calculate(&job.query, job.deadline, Some(&job.cancellation));
                if job.cancellation.is_cancelled() {
                    continue;
                }
                if input
                    .try_send(Input::Calculation(job.query.id, result))
                    .is_err()
                {
                    overflow.store(true, Ordering::Release);
                    break;
                }
            }
        })?;
        Ok(Self {
            jobs,
            failed,
            thread,
        })
    }

    pub(super) fn submit(&self, job: Job) -> bool {
        self.jobs.try_send(job).is_ok()
    }

    pub(super) fn failed(&self) -> bool {
        self.failed.load(Ordering::Acquire)
    }

    pub(super) fn stop(self) {
        drop(self.jobs);
        let _ = self.thread.join();
    }
}
