//! Serializes every IPC call into KiCAD behind a per-address FIFO.
//!
//! KiCAD's API server runs on the UI thread. Before this module, every
//! `with_ipc` call left independently on `spawn_blocking`, so N concurrent
//! tool handlers could hit KiCAD in parallel. `place_footprint`
//! (`konnect-ipc/src/client.rs`) is the concrete failure this fixes: it does
//! a read-modify-write in four commands (find_open_board → list_footprints_in
//! → create_items_in → list_footprints_in) guarded by a "does this reference
//! already exist" check. Two concurrent calls can both pass that check and
//! create a duplicate footprint. Serializing the whole call sequence per
//! address is the only fix that doesn't require KiCAD itself to grow
//! transactions.
//!
//! # Design decisions
//!
//! - **No retry, ever.** A job here is an `FnOnce` closure whose effects are
//!   not observable from outside; replaying it after a partial failure is
//!   exactly the double-apply this module exists to prevent. The only retry
//!   that is safe is the caller's own retry with an idempotency key, already
//!   served by `kam_state::IdempotencyLedger`.
//! - **No timeout added here.** Every individual command is already bounded
//!   by `KiCadIpcClient::send_command`'s `SendTimeout`/`RecvTimeout`, and a
//!   job sends a finite number of commands. A queue-level timeout would turn
//!   "this is slow" into "I no longer know whether this applied," which is
//!   strictly worse.
//! - **The key is the IPC address**, because the address names the KiCAD
//!   instance being serialized against. Two different addresses have no
//!   reason to block each other, and keying by address (not a shared global)
//!   is what keeps tests independent without a shared environment variable.
//! - **Re-entrant deadlock is structurally impossible.** A queued job's
//!   closure only ever receives a `&KiCadIpcClient`, which is synchronous and
//!   has no path back into `with_ipc` (async). A job cannot enqueue another
//!   job and wait on it.

use konnect_ipc::client::KiCadIpcClient;
use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::sync::{Mutex, OnceLock};

/// One unit of work destined for a specific address's worker thread. Boxed as
/// `FnOnce` because that's what erases the handler's own `T`; the result is
/// delivered back through the paired `oneshot`.
type Job = Box<dyn FnOnce(&KiCadIpcClient) + Send + 'static>;

/// Process-global registry of per-address queues, populated lazily: the first
/// `submit` for an address spins up its worker thread, every later `submit`
/// for the same address reuses it.
static QUEUES: OnceLock<Mutex<HashMap<String, std::sync::mpsc::Sender<Job>>>> = OnceLock::new();

fn registry() -> &'static Mutex<HashMap<String, std::sync::mpsc::Sender<Job>>> {
    QUEUES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Get (creating if absent) the job sender for `addr`.
///
/// Creating the worker thread happens inside the registry lock so two
/// concurrent first-callers for the same address can't each spawn a thread;
/// the lock is held only long enough to insert the channel, never across a
/// job's execution.
fn sender_for(addr: &str) -> std::sync::mpsc::Sender<Job> {
    let mut queues = registry().lock().unwrap();
    if let Some(tx) = queues.get(addr) {
        return tx.clone();
    }
    let (tx, rx) = std::sync::mpsc::channel::<Job>();
    let thread_addr = addr.to_string();
    // Named for jstack/Process Explorer legibility when several KiCAD
    // instances are being driven at once.
    std::thread::Builder::new()
        .name(format!("konnect-ipc-{thread_addr}"))
        .spawn(move || {
            // One client per worker thread, built once and reused for every
            // job on this address — `KiCadIpcClient::new` is infallible and
            // holds no durable state, so there is nothing to invalidate.
            let client = KiCadIpcClient::new(&thread_addr);
            for job in rx {
                // A panicking job must not take the worker thread down with
                // it: that would silently wedge every job submitted after it
                // for the rest of the process. `AssertUnwindSafe` is sound
                // here because `job` closes over its own oneshot sender and
                // whatever state it needs — nothing shared survives a panic
                // for a later job to observe in a torn state.
                let _ = std::panic::catch_unwind(AssertUnwindSafe(|| job(&client)));
            }
        })
        .expect("failed to spawn konnect-ipc worker thread");
    queues.insert(addr.to_string(), tx.clone());
    tx
}

/// Run `f` against the `KiCadIpcClient` serialized behind `addr`'s worker
/// thread, returning a future that resolves once `f` has run (or the worker
/// panicked while running it).
///
/// Submission into the queue happens synchronously, before the returned
/// future is ever polled, so the FIFO order is the order this function was
/// *called*, not the order the caller happened to poll its future — the
/// property `with_ipc` depends on to keep concurrent calls from interleaving.
///
/// The outer `Err` is reserved for queue/thread failure (a panicked job, or a
/// worker thread that vanished): a bug in this process, never a statement
/// about KiCAD.
pub(crate) fn submit<T, F>(addr: &str, f: F) -> impl std::future::Future<Output = anyhow::Result<T>>
where
    T: Send + 'static,
    F: FnOnce(&KiCadIpcClient) -> T + Send + 'static,
{
    let (tx, rx) = tokio::sync::oneshot::channel::<T>();
    let sender = sender_for(addr);
    let job: Job = Box::new(move |client| {
        let result = f(client);
        // Ignored: the caller dropped its future (e.g. the request was
        // cancelled) before the job ran. The job still ran to completion —
        // there is nothing left to signal, and nothing to undo.
        let _ = tx.send(result);
    });
    // Submission is synchronous and happens here, before this function
    // returns a future — the FIFO order this whole module exists for.
    let submitted = sender.send(job).is_ok();
    async move {
        if !submitted {
            anyhow::bail!("IPC worker thread for this address is no longer running");
        }
        rx.await
            .map_err(|_| anyhow::anyhow!("IPC job panicked before producing a result"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[tokio::test]
    async fn ipc_jobs_never_overlap() {
        let addr = "ipc_jobs_never_overlap";
        let concurrent = Arc::new(AtomicUsize::new(0));
        let max_seen = Arc::new(AtomicUsize::new(0));

        let mut handles = Vec::new();
        for _ in 0..8 {
            let concurrent = concurrent.clone();
            let max_seen = max_seen.clone();
            handles.push(tokio::spawn(async move {
                submit(addr, move |_client| {
                    let now = concurrent.fetch_add(1, Ordering::SeqCst) + 1;
                    max_seen.fetch_max(now, Ordering::SeqCst);
                    std::thread::sleep(std::time::Duration::from_millis(20));
                    concurrent.fetch_sub(1, Ordering::SeqCst);
                })
                .await
                .unwrap();
            }));
        }
        for h in handles {
            h.await.unwrap();
        }

        assert_eq!(max_seen.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn ipc_jobs_run_in_submission_order() {
        let addr = "ipc_jobs_run_in_submission_order";
        let order: Arc<Mutex<Vec<usize>>> = Arc::new(Mutex::new(Vec::new()));

        // Submission is synchronous, so calling `submit` in a plain loop
        // (not spawning) already fixes the order; the futures are then
        // awaited concurrently to also prove polling order doesn't matter.
        let mut futures = Vec::new();
        for i in 0..8 {
            let order = order.clone();
            futures.push(submit(addr, move |_client| {
                order.lock().unwrap().push(i);
            }));
        }
        for fut in futures {
            fut.await.unwrap();
        }

        assert_eq!(*order.lock().unwrap(), (0..8).collect::<Vec<_>>());
    }

    #[tokio::test]
    async fn a_panicking_ipc_job_does_not_wedge_the_queue() {
        let addr = "a_panicking_ipc_job_does_not_wedge_the_queue";

        let panicking = submit(addr, |_client| -> () {
            panic!("simulated handler panic");
        })
        .await;
        assert!(panicking.is_err());

        let after = submit(addr, |_client| 42).await.unwrap();
        assert_eq!(after, 42);
    }

    #[tokio::test]
    async fn distinct_ipc_addresses_do_not_serialise_against_each_other() {
        let concurrent = Arc::new(AtomicUsize::new(0));
        let max_seen = Arc::new(AtomicUsize::new(0));

        let mut handles = Vec::new();
        for addr in [
            "distinct_ipc_addresses_do_not_serialise_against_each_other_a",
            "distinct_ipc_addresses_do_not_serialise_against_each_other_b",
        ] {
            let concurrent = concurrent.clone();
            let max_seen = max_seen.clone();
            handles.push(tokio::spawn(async move {
                submit(addr, move |_client| {
                    let now = concurrent.fetch_add(1, Ordering::SeqCst) + 1;
                    max_seen.fetch_max(now, Ordering::SeqCst);
                    // 200 ms rather than the 20 ms of the serialization test:
                    // that one fails when the queue is broken, this one fails
                    // when the *machine* is slow, so it needs the wider window.
                    std::thread::sleep(std::time::Duration::from_millis(200));
                    concurrent.fetch_sub(1, Ordering::SeqCst);
                })
                .await
                .unwrap();
            }));
        }
        for h in handles {
            h.await.unwrap();
        }

        assert_eq!(max_seen.load(Ordering::SeqCst), 2);
    }
}
