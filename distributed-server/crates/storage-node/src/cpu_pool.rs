//! Bounded pool for CPU-bound work run off the Tokio worker threads.

use core::num::NonZeroUsize;
use std::sync::Arc;

use tokio::sync::Semaphore;
use tokio::task::spawn_blocking;

/// Failure modes of the CPU pool.
#[derive(Debug, thiserror::Error)]
pub enum CpuPoolError {
    #[error("the CPU pool is closed")]
    Closed,
    #[error("the CPU task failed")]
    Task(#[from] tokio::task::JoinError),
}

type Result<T> = core::result::Result<T, CpuPoolError>;

/// Bounded pool running CPU-bound closures on blocking threads.
#[derive(Clone)]
pub struct CpuPool {
    permits: Arc<Semaphore>,
}

impl CpuPool {
    /// Creates a pool that runs at most `max_concurrency` CPU tasks at once.
    pub fn new(max_concurrency: NonZeroUsize) -> Self {
        Self {
            permits: Arc::new(Semaphore::new(max_concurrency.get())),
        }
    }

    /// Runs `work` off the Tokio worker threads, waiting for a free slot.
    pub async fn run<F, T>(&self, work: F) -> Result<T>
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        let permit = self
            .permits
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| CpuPoolError::Closed)?;
        let joined = spawn_blocking(work).await?;
        drop(permit);
        Ok(joined)
    }
}
