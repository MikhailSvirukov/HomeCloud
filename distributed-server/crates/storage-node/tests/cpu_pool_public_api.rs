//! Integration tests exercising only the public CPU pool API ([`CpuPool`]).

use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

use storage_node::{CpuPool, CpuPoolError};

#[tokio::test]
async fn bounds_concurrency_to_the_configured_limit() {
    let pool = CpuPool::new(NonZeroUsize::new(1).expect("one permit is non-zero"));
    let started = Arc::new(AtomicUsize::new(0));
    let second_ran = Arc::new(AtomicBool::new(false));
    let (release_tx, release_rx) = channel::<()>();

    let first = tokio::spawn(block_on_first(
        pool.clone(),
        Arc::clone(&started),
        release_rx,
    ));

    while started.load(Ordering::SeqCst) == 0 {
        tokio::task::yield_now().await;
    }

    let second = tokio::spawn(run_second(pool.clone(), Arc::clone(&second_ran)));

    for _ in 0..32 {
        tokio::task::yield_now().await;
    }

    assert!(
        !second_ran.load(Ordering::SeqCst),
        "the second task must not start while the single permit is held"
    );

    release_tx
        .send(())
        .expect("the first task still holds its release receiver");

    first
        .await
        .expect("the first task joins")
        .expect("the pool runs the first task");
    second
        .await
        .expect("the second task joins")
        .expect("the pool runs the second task");

    assert!(
        second_ran.load(Ordering::SeqCst),
        "the second task must run once the permit is released"
    );
}

#[tokio::test]
async fn cancelling_run_keeps_the_permit_with_the_running_task() {
    let pool = CpuPool::new(NonZeroUsize::new(1).expect("one permit is non-zero"));
    let started = Arc::new(AtomicUsize::new(0));
    let second_started = Arc::new(AtomicBool::new(false));
    let (release_tx, release_rx) = channel::<()>();

    let first = tokio::spawn(block_on_first(
        pool.clone(),
        Arc::clone(&started),
        release_rx,
    ));

    while started.load(Ordering::SeqCst) == 0 {
        tokio::task::yield_now().await;
    }

    first.abort();

    let second = tokio::spawn(run_second(pool.clone(), Arc::clone(&second_started)));

    for _ in 0..32 {
        tokio::task::yield_now().await;
    }

    assert!(
        !second_started.load(Ordering::SeqCst),
        "the second task must not start while the cancelled task's closure holds the permit"
    );

    release_tx
        .send(())
        .expect("the first task's closure still holds its release receiver");

    second
        .await
        .expect("the second task joins")
        .expect("the pool runs the second task");

    assert!(
        second_started.load(Ordering::SeqCst),
        "the second task must run once the cancelled task's closure releases the permit"
    );
}

#[tokio::test]
async fn runs_many_tasks_and_returns_results() {
    let pool = CpuPool::new(NonZeroUsize::new(2).expect("two permits are non-zero"));

    let handles: Vec<_> = (0..6usize)
        .map(|i| {
            let pool = pool.clone();
            tokio::spawn(async move { pool.run(move || i * i).await })
        })
        .collect();

    for (i, handle) in handles.into_iter().enumerate() {
        let result = handle
            .await
            .expect("the task joins")
            .expect("the pool runs the task");

        assert_eq!(result, i * i);
    }
}

#[tokio::test]
async fn closing_the_pool_rejects_new_runs() {
    let pool = CpuPool::new(NonZeroUsize::new(2).expect("two permits are non-zero"));
    pool.close();

    let outcome = pool.run(|| 1).await;

    assert!(matches!(outcome, Err(CpuPoolError::Closed)));
}

async fn block_on_first(
    pool: CpuPool,
    started: Arc<AtomicUsize>,
    release_rx: Receiver<()>,
) -> Result<(), CpuPoolError> {
    pool.run(move || {
        started.fetch_add(1, Ordering::SeqCst);
        release_rx
            .recv()
            .expect("the first task is released before the receiver drops");
    })
    .await
}

async fn run_second(pool: CpuPool, second_ran: Arc<AtomicBool>) -> Result<(), CpuPoolError> {
    pool.run(move || {
        second_ran.store(true, Ordering::SeqCst);
    })
    .await
}
