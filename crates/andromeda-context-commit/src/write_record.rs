//! The idempotency record one atomic write owns.

use crate::write_id::WriteId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// The identity and ingestion instant that one atomic write is replayed under.
///
/// Both halves are durable and both are reused on replay. The identity alone is
/// not enough: history rows deduplicate on a key that also contains the
/// partition column `recorded_at`, so a replay that re-stamped the ingestion
/// instant would land beside the rows it means to collide with and duplicate
/// them. Persisting the pair, and reading the pair back, is what makes a
/// repeated append a no-op.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct WriteRecord {
    /// Identity correlating every durable consequence of the write.
    write_id: WriteId,
    /// Server-side instant the write was accepted for durable storage.
    recorded_at: DateTime<Utc>,
}

impl WriteRecord {
    /// Open the record of a write being attempted for the first time.
    #[must_use]
    pub fn mint(recorded_at: DateTime<Utc>) -> WriteRecord {
        WriteRecord {
            write_id: WriteId::mint(),
            recorded_at,
        }
    }

    /// Reconstruct the record of a write read back from durable storage.
    #[must_use]
    pub const fn restored(write_id: WriteId, recorded_at: DateTime<Utc>) -> WriteRecord {
        WriteRecord { write_id, recorded_at }
    }

    /// Return the identity every consequence of this write carries.
    #[must_use]
    pub const fn write_id(&self) -> WriteId {
        self.write_id
    }

    /// Return the ingestion instant every consequence of this write shares.
    #[must_use]
    pub const fn recorded_at(&self) -> DateTime<Utc> {
        self.recorded_at
    }
}

#[cfg(test)]
mod tests {
    use crate::{write_id::WriteId, write_record::WriteRecord};
    use chrono::{TimeZone, Utc};

    #[test]
    fn minting_a_record_gives_each_write_its_own_identity() {
        let recorded_at = Utc.timestamp_opt(1_700_000_000, 0).single().expect("valid instant");
        let first = WriteRecord::mint(recorded_at);
        let second = WriteRecord::mint(recorded_at);
        assert_ne!(first.write_id(), second.write_id());
        assert_eq!(first.recorded_at(), second.recorded_at());
    }

    #[test]
    fn a_restored_record_reuses_both_persisted_halves() {
        let recorded_at = Utc.timestamp_opt(1_700_000_000, 0).single().expect("valid instant");
        let minted = WriteRecord::mint(recorded_at);
        let restored = WriteRecord::restored(minted.write_id(), minted.recorded_at());
        assert_eq!(restored, minted);
    }

    #[test]
    fn a_restored_record_does_not_mint_a_new_identity() {
        let recorded_at = Utc.timestamp_opt(1_700_000_000, 0).single().expect("valid instant");
        let write_id = WriteId::mint();
        assert_eq!(WriteRecord::restored(write_id, recorded_at).write_id(), write_id);
    }
}
