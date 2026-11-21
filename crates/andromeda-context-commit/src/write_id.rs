//! The identity of one atomic broker write.

use derive_more::Display;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Identity of one atomic broker write.
///
/// The same value correlates every durable consequence of that write: its
/// temporal history rows, its notification events and its deferred
/// side-effect jobs. It is a version-7 UUID so durable rows keyed by it are
/// stored in write order, which keeps the index that carries it append-mostly
/// instead of scattering inserts across its whole key space.
#[derive(Clone, Copy, Debug, Deserialize, Display, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[display("{_0}")]
pub struct WriteId(Uuid);

impl WriteId {
    /// Mint the identity of a write that is about to be attempted.
    #[must_use]
    pub fn mint() -> WriteId {
        WriteId(Uuid::now_v7())
    }

    /// Reconstruct an identity read back from durable storage.
    ///
    /// A replay must reuse the stored identity rather than mint a new one, or
    /// it writes rows that collide with nothing and duplicate history.
    #[must_use]
    pub const fn from_uuid(value: Uuid) -> WriteId {
        WriteId(value)
    }

    /// Return the persistence representation.
    #[must_use]
    pub const fn as_uuid(self) -> Uuid {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use crate::write_id::WriteId;
    use pretty_assertions::assert_eq;
    use std::{array, collections::HashSet, thread};

    /// Threads minting at once.
    const THREADS: usize = 8;

    /// Identities each thread mints.
    const PER_THREAD: usize = 256;

    #[test]
    fn minted_identities_are_version_seven() {
        assert_eq!(WriteId::mint().as_uuid().get_version_num(), 7);
    }

    #[test]
    fn minted_identities_carry_the_rfc_9562_variant() {
        // RFC 9562 section 4.1: the variant bits of the 9th octet are 0b10xx.
        let octet = WriteId::mint().as_uuid().as_bytes()[8];
        assert_eq!(octet & 0b1100_0000, 0b1000_0000);
    }

    #[test]
    fn identities_minted_in_one_millisecond_keep_mint_order() {
        let minted: Vec<WriteId> = (0..1024).map(|_| WriteId::mint()).collect();
        let mut sorted = minted.clone();
        sorted.sort_unstable();
        assert_eq!(minted, sorted);
    }

    #[test]
    fn identities_minted_in_one_millisecond_are_distinct() {
        let minted: HashSet<WriteId> = (0..1024).map(|_| WriteId::mint()).collect();
        assert_eq!(minted.len(), 1024);
    }

    #[test]
    fn identities_minted_concurrently_are_distinct_and_keep_each_thread_in_order() {
        // Every thread is spawned before any is joined, so the mints contend
        // for the one process-wide counter.
        let handles: [_; THREADS] = array::from_fn(|_| thread::spawn(|| (0..PER_THREAD).map(|_| WriteId::mint()).collect::<Vec<_>>()));
        let batches = handles.map(|handle| handle.join().expect("a minting thread"));
        assert!(batches.iter().all(|batch| batch.is_sorted()), "one thread's mints went backwards");
        let minted: HashSet<WriteId> = batches.iter().flatten().copied().collect();
        assert_eq!(minted.len(), THREADS * PER_THREAD);
    }

    #[test]
    fn a_restored_identity_equals_the_identity_it_was_read_from() {
        let minted = WriteId::mint();
        assert_eq!(WriteId::from_uuid(minted.as_uuid()), minted);
    }
}
