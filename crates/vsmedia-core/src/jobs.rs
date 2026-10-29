//! Bounded cooperative worker pool for offline processing (renders, pack builds).
use std::{
    collections::HashMap,
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
};
use vsmedia_api::{MediaError, MediaResult};

pub type JobId = u64;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus { Queued, Running, Completed, Cancelled, Failed }
#[derive(Debug, Clone)]
pub struct JobSnapshot {
    pub id: JobId,
    pub status: JobStatus,
    pub error: Option<String>,
}
#[derive(Clone, Debug)]
pub struct CancelToken(Arc<AtomicBool>);
impl CancelToken {
    pub fn is_cancelled(&self) -> bool { self.0.load(Ordering::Acquire) }
}

type JobFn = Box<dyn FnOnce(CancelToken) -> Result<(), String> + Send + 'static>;
struct WorkItem { id: JobId, run: JobFn, cancel: CancelToken }
struct State {
    records: Mutex<HashMap<JobId, (JobSnapshot, CancelToken)>>,
    sequence: AtomicU64,
}
pub struct JobManager {
    state: Arc<State>,
    sender: Option<SyncSender<WorkItem>>,
    workers: Vec<JoinHandle<()>>,
}
impl JobManager {
    pub fn new(worker_count: usize, queue_capacity: usize) -> MediaResult<Self> {
        if !(1..=32).contains(&worker_count) || !(1..=1024).contains(&queue_capacity) {
            return Err(MediaError::InvalidInput("invalid worker pool limits".into()));
        }
        let (sender, receiver) = mpsc::sync_channel::<WorkItem>(queue_capacity);
        let receiver = Arc::new(Mutex::new(receiver));
        let state = Arc::new(State { records: Mutex::new(HashMap::new()), sequence: AtomicU64::new(0) });
        let workers = (0..worker_count).map(|_| {
            let state = Arc::clone(&state);
            let receiver = Arc::clone(&receiver);
            thread::spawn(move || worker_loop(&receiver, &state))
        }).collect();
        Ok(Self { state, sender: Some(sender), workers })
    }
    pub fn submit<F>(&self, job: F) -> MediaResult<JobId>
    where F: FnOnce(CancelToken) -> Result<(), String> + Send + 'static {
        let id = self.state.sequence.fetch_add(1, Ordering::Relaxed) + 1;
        let token = CancelToken(Arc::new(AtomicBool::new(false)));
        self.state.records.lock().map_err(lock_error)?.insert(id, (
            JobSnapshot { id, status: JobStatus::Queued, error: None }, token.clone()
        ));
        let item = WorkItem { id, run: Box::new(job), cancel: token };
        if let Err(error) = self.sender.as_ref().expect("sender exists until drop").try_send(item) {
            self.state.records.lock().map_err(lock_error)?.remove(&id);
            return Err(MediaError::Conflict(format!("job queue rejected submission: {error}")));
        }
        Ok(id)
    }
    pub fn cancel(&self, id: JobId) -> MediaResult<()> {
        let mut records = self.state.records.lock().map_err(lock_error)?;
        let (snapshot, token) = records.get_mut(&id).ok_or_else(|| MediaError::NotFound(format!("job {id}")))?;
        token.0.store(true, Ordering::Release);
        if snapshot.status == JobStatus::Queued { snapshot.status = JobStatus::Cancelled; }
        Ok(())
    }
    pub fn status(&self, id: JobId) -> MediaResult<JobSnapshot> {
        self.state.records.lock().map_err(lock_error)?
            .get(&id).map(|r| r.0.clone()).ok_or_else(|| MediaError::NotFound(format!("job {id}")))
    }
}
fn worker_loop(receiver: &Mutex<Receiver<WorkItem>>, state: &State) {
    loop {
        let item = match receiver.lock().expect("queue mutex").recv() {
            Ok(item) => item,
            Err(_) => break,
        };
        if item.cancel.is_cancelled() { continue; }
        {
            let mut records = state.records.lock().expect("records mutex");
            if let Some((record, _)) = records.get_mut(&item.id) { record.status = JobStatus::Running; }
        }
        let result = catch_unwind(AssertUnwindSafe(|| (item.run)(item.cancel.clone())));
        let mut records = state.records.lock().expect("records mutex");
        if let Some((record, _)) = records.get_mut(&item.id) {
            if item.cancel.is_cancelled() {
                record.status = JobStatus::Cancelled;
            } else {
                match result {
                    Ok(Ok(())) => record.status = JobStatus::Completed,
                    Ok(Err(error)) => { record.status = JobStatus::Failed; record.error = Some(error); }
                    Err(_) => { record.status = JobStatus::Failed; record.error = Some("worker panicked".into()); }
                }
            }
        }
    }
}
fn lock_error<E: std::fmt::Display>(e: E) -> MediaError { MediaError::Io(e.to_string()) }
impl Drop for JobManager {
    fn drop(&mut self) {
        self.sender.take();
        for worker in self.workers.drain(..) { let _ = worker.join(); }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn completes_tasks_and_catches_panics() {
        let manager = JobManager::new(1, 5).unwrap();
        let one = manager.submit(|_| Ok(())).unwrap();
        let two = manager.submit(|_| panic!("expected test panic")).unwrap();
        for _ in 0..100 {
            if manager.status(two).unwrap().status == JobStatus::Failed { break; }
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(manager.status(one).unwrap().status, JobStatus::Completed);
        assert_eq!(manager.status(two).unwrap().status, JobStatus::Failed);
    }
    #[test]
    fn cancel_token_can_be_observed() {
        let token = CancelToken(Arc::new(AtomicBool::new(false)));
        assert!(!token.is_cancelled());
        token.0.store(true, Ordering::Release);
        assert!(token.is_cancelled());
    }
}
