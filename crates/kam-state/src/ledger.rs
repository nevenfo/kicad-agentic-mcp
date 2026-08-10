//! Idempotency ledger for batched mutations.
//!
//! The failure this prevents is not exotic: a client times out waiting for a
//! batch, retries it, and the second run adds the same three capacitors again.
//! The transport cannot tell the two calls apart — only a key supplied by the
//! caller can, so the caller supplies one and the ledger remembers the outcome
//! it already produced for it.
//!
//! Entries are held in memory for the life of the process and bounded by
//! `capacity`; a retry that arrives after that many *other* operations is
//! treated as fresh. That is the honest trade: durability across a restart
//! would need a journal on disk, and the window this protects (a client
//! retrying a call it just made) is measured in seconds.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

/// What a caller should do with an operation key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Claim<T> {
    /// Never seen. The caller owns it and must finish with
    /// [`IdempotencyLedger::complete`] or [`IdempotencyLedger::abandon`].
    Fresh,
    /// Already completed. Return this result instead of running anything.
    Replay(T),
    /// Claimed by another in-flight call. Retrying blind would double-apply.
    InFlight,
}

#[derive(Debug)]
enum Entry<T> {
    InFlight,
    Done(T),
}

#[derive(Debug)]
struct Inner<T> {
    entries: HashMap<String, Entry<T>>,
    order: VecDeque<String>,
}

/// Remembers the result of each completed operation key.
#[derive(Debug)]
pub struct IdempotencyLedger<T> {
    inner: Mutex<Inner<T>>,
    capacity: usize,
}

impl<T: Clone> Default for IdempotencyLedger<T> {
    fn default() -> Self {
        Self::new(256)
    }
}

impl<T: Clone> IdempotencyLedger<T> {
    /// A ledger holding at most `capacity` keys.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Mutex::new(Inner {
                entries: HashMap::new(),
                order: VecDeque::new(),
            }),
            capacity: capacity.max(1),
        }
    }

    /// Claim `id`, or learn that it is already running or already done.
    pub fn claim(&self, id: &str) -> Claim<T> {
        let mut inner = self.lock();
        match inner.entries.get(id) {
            Some(Entry::Done(value)) => Claim::Replay(value.clone()),
            Some(Entry::InFlight) => Claim::InFlight,
            None => {
                inner.entries.insert(id.to_string(), Entry::InFlight);
                inner.order.push_back(id.to_string());
                self.evict(&mut inner);
                Claim::Fresh
            }
        }
    }

    /// Record the result of a claimed operation so a replay returns it.
    pub fn complete(&self, id: &str, value: T) {
        let mut inner = self.lock();
        if let Some(slot) = inner.entries.get_mut(id) {
            *slot = Entry::Done(value);
        }
    }

    /// Release a claim without recording a result — the operation never ran, so
    /// a later retry must be allowed to run it.
    pub fn abandon(&self, id: &str) {
        let mut inner = self.lock();
        inner.entries.remove(id);
        inner.order.retain(|k| k != id);
    }

    /// Number of keys currently remembered.
    pub fn len(&self) -> usize {
        self.lock().entries.len()
    }

    /// Whether the ledger is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// A poisoned ledger is not a reason to fail a KiCAD edit: the data behind
    /// the mutex is a cache of past results, and the worst case of using it
    /// after a panic elsewhere is a replay that re-runs.
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner<T>> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Drop oldest completed keys past capacity. In-flight keys are never
    /// evicted — forgetting one is exactly the double-apply this exists to stop.
    fn evict(&self, inner: &mut Inner<T>) {
        let mut kept: Vec<String> = Vec::new();
        while inner.entries.len() > self.capacity {
            let Some(oldest) = inner.order.pop_front() else {
                break;
            };
            match inner.entries.get(&oldest) {
                Some(Entry::Done(_)) => {
                    inner.entries.remove(&oldest);
                }
                Some(Entry::InFlight) => kept.push(oldest),
                None => {}
            }
        }
        for id in kept.into_iter().rev() {
            inner.order.push_front(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_claim_is_fresh_and_replays_after_completion() {
        let ledger: IdempotencyLedger<String> = IdempotencyLedger::new(8);
        assert_eq!(ledger.claim("op_1"), Claim::Fresh);
        ledger.complete("op_1", "applied 3 components".to_string());
        assert_eq!(
            ledger.claim("op_1"),
            Claim::Replay("applied 3 components".to_string())
        );
    }

    #[test]
    fn a_second_claim_before_completion_is_in_flight() {
        let ledger: IdempotencyLedger<u32> = IdempotencyLedger::new(8);
        assert_eq!(ledger.claim("op_2"), Claim::Fresh);
        assert_eq!(ledger.claim("op_2"), Claim::InFlight);
    }

    #[test]
    fn abandon_lets_a_retry_run() {
        let ledger: IdempotencyLedger<u32> = IdempotencyLedger::new(8);
        assert_eq!(ledger.claim("op_3"), Claim::Fresh);
        ledger.abandon("op_3");
        assert_eq!(ledger.claim("op_3"), Claim::Fresh);
        assert!(ledger.is_empty() || ledger.len() == 1);
    }

    #[test]
    fn capacity_evicts_completed_keys_oldest_first() {
        let ledger: IdempotencyLedger<u32> = IdempotencyLedger::new(2);
        for i in 0..4u32 {
            let id = format!("op_{i}");
            assert_eq!(ledger.claim(&id), Claim::Fresh);
            ledger.complete(&id, i);
        }
        assert!(ledger.len() <= 2);
        assert_eq!(ledger.claim("op_0"), Claim::Fresh, "oldest was evicted");
        assert_eq!(ledger.claim("op_3"), Claim::Replay(3), "newest survived");
    }

    #[test]
    fn in_flight_keys_are_never_evicted() {
        let ledger: IdempotencyLedger<u32> = IdempotencyLedger::new(1);
        assert_eq!(ledger.claim("long_running"), Claim::Fresh);
        for i in 0..5u32 {
            let id = format!("op_{i}");
            ledger.claim(&id);
            ledger.complete(&id, i);
        }
        assert_eq!(
            ledger.claim("long_running"),
            Claim::InFlight,
            "an in-flight claim must survive eviction pressure"
        );
    }
}
