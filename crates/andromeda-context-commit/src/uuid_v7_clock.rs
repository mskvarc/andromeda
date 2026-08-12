//! The process-wide clock every broker-minted version-7 identity comes from.

use std::sync::{Mutex, PoisonError};
use uuid::{ContextV7, Timestamp, Uuid};

/// Process-wide monotonic counter shared by every minted version-7 identity.
///
/// Version-7 UUIDs only sort by mint order when one counter observes every
/// mint: two independent contexts can each hand out a value for the same
/// millisecond and produce a pair whose order contradicts the order in which
/// the values were handed out. `ContextV7` keeps its counter in a `Cell` and is
/// therefore not `Sync`, so the single counter is reached under a lock.
static UUID_V7_CLOCK: Mutex<ContextV7> = Mutex::new(ContextV7::new());

/// Mint the next version-7 UUID of this process.
pub(crate) fn mint() -> Uuid {
    // A poisoned counter is still a counter: the panic that poisoned it
    // happened elsewhere, and refusing to mint identities afterwards would
    // turn one unrelated failure into a broker that accepts no writes.
    let clock = UUID_V7_CLOCK.lock().unwrap_or_else(PoisonError::into_inner);
    Uuid::new_v7(Timestamp::now(&*clock))
}

#[cfg(test)]
mod tests {
    use crate::uuid_v7_clock;
    use std::{collections::HashSet, thread};
    use uuid::{ContextV7, Timestamp, Uuid};

    /// Generators minting concurrently against the process-wide counter.
    const GENERATORS: usize = 8;

    /// Values each concurrent generator mints.
    const PER_GENERATOR: usize = 256;

    /// The RFC 9562 section 4.1 variant bits of the ninth octet.
    fn is_rfc_9562_variant(value: Uuid) -> bool {
        value.as_bytes()[8] & 0b1100_0000 == 0b1000_0000
    }

    #[test]
    fn every_minted_value_is_a_version_seven_uuid() {
        assert_eq!(uuid_v7_clock::mint().get_version_num(), 7);
    }

    #[test]
    fn every_minted_value_carries_the_rfc_9562_variant() {
        assert!(is_rfc_9562_variant(uuid_v7_clock::mint()));
    }

    #[test]
    fn values_minted_within_one_millisecond_keep_mint_order_and_stay_distinct() {
        let minted: Vec<uuid::Uuid> = (0..1024).map(|_| uuid_v7_clock::mint()).collect();
        let mut sorted = minted.clone(); // clone: the comparison needs both the mint order and the sorted order.
        sorted.sort_unstable();
        assert_eq!(minted, sorted);
        assert_eq!(minted.iter().collect::<HashSet<_>>().len(), 1024);
    }

    #[test]
    fn values_minted_concurrently_stay_distinct_and_keep_each_generator_in_order() {
        // Every generator is started before any is joined, so the mints
        // genuinely contend for the process-wide counter.
        let mut generators = Vec::with_capacity(GENERATORS);
        for _ in 0..GENERATORS {
            generators.push(thread::spawn(|| (0..PER_GENERATOR).map(|_| uuid_v7_clock::mint()).collect::<Vec<Uuid>>()));
        }
        let mut batches = Vec::with_capacity(GENERATORS);
        for generator in generators {
            match generator.join() {
                Ok(batch) => batches.push(batch),
                Err(_) => unreachable!("a minting generator panicked"),
            }
        }

        for batch in &batches {
            let mut sorted = batch.clone(); // clone: the comparison needs both the mint order and the sorted order.
            sorted.sort_unstable();
            assert_eq!(batch, &sorted, "one generator's own mints must not go backwards");
        }

        let minted: HashSet<Uuid> = batches.iter().flatten().copied().collect();
        assert_eq!(minted.len(), GENERATORS * PER_GENERATOR, "concurrent generators must not mint one value twice");
        assert!(minted.iter().all(|value| value.get_version_num() == 7));
        assert!(minted.iter().copied().all(is_rfc_9562_variant));
    }

    /// A backwards system clock is the condition that would let a version-7
    /// value be minted before one already handed out. The monotonic context this
    /// module mints through carries the last instant it observed, so a
    /// rolled-back reading neither repeats an identity nor reorders one against
    /// the values minted before the rollback.
    #[test]
    fn a_rolled_back_clock_neither_repeats_nor_reorders_an_identity() {
        let context = ContextV7::new();
        let before_rollback = Uuid::new_v7(Timestamp::from_unix(&context, 1_700_000_001, 0));
        let after_rollback: Vec<Uuid> = (0..64).map(|_| Uuid::new_v7(Timestamp::from_unix(&context, 1_700_000_000, 0))).collect();

        assert!(after_rollback.iter().all(|value| value.get_version_num() == 7));
        assert!(after_rollback.iter().copied().all(is_rfc_9562_variant));
        let minted: HashSet<Uuid> = after_rollback.iter().copied().chain(std::iter::once(before_rollback)).collect();
        assert_eq!(minted.len(), 65, "a rolled-back clock must not repeat an identity");
        assert!(
            after_rollback.iter().all(|value| *value > before_rollback),
            "a rolled-back clock reading must not mint a value that sorts before one already handed out"
        );
        let mut ordered = after_rollback.clone(); // clone: the comparison needs both the mint order and the sorted order.
        ordered.sort_unstable();
        assert_eq!(after_rollback, ordered, "mints under a rolled-back clock keep their own order");
    }
}
