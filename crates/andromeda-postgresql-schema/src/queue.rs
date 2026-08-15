//! The relations that record write identity and deferred work.
//!
//! `write_records` is one write's idempotency bookkeeping row, persisted once
//! per write so a replay reuses the original `recorded_at` instant that the
//! hypertable rows deduplicate on. `pending_side_effects` is the durable queue
//! of deferred batch jobs a committing write enqueues, drained at-least-once by
//! workers that lease rows under a time-bounded claim.

use sea_query::Iden;

/// The `write_records` table - one idempotency record per write.
#[derive(Iden, Clone, Copy)]
pub enum WriteRecords {
    /// Table name: `write_records`
    Table,
    /// Write identity (primary key)
    WriteId,
    /// Instant the write's effects are stamped with, reused on replay
    RecordedAt,
    /// Instant the broker accepted the write, moved on each replay
    AcceptedAt,
}

/// The `pending_side_effects` table - the durable deferred-job queue.
#[derive(Iden, Clone, Copy)]
pub enum PendingSideEffects {
    /// Table name: `pending_side_effects`
    Table,
    /// Job surrogate identity, minted by the enqueue
    Id,
    /// Identity of the write that enqueued the job, the deduplication key
    WriteId,
    /// Instant the enqueueing write stamped, reused when the job resumes
    RecordedAt,
    /// Ordinal base the enqueueing write's consequences resume from
    ConsequenceBase,
    /// Serialised batch side-effect job
    Payload,
    /// Number of drain attempts already made
    Attempts,
    /// Whether the job is retired and no longer drainable
    Dead,
    /// Diagnostic recorded by the last failed attempt
    LastError,
    /// Earliest instant the job may next be attempted
    NextAttemptAt,
    /// Instant the current lease expires
    LockedUntil,
    /// Worker that holds the current lease
    LockedBy,
}

/// The `temporal_append_intents` table - the durable temporal-append queue.
#[derive(Iden, Clone, Copy)]
pub enum TemporalAppendIntents {
    /// Table name: `temporal_append_intents`
    Table,
    /// Monotonic queue sequence, the row identity workers order and claim by
    Sequence,
    /// Identity of the write that produced the intent
    WriteId,
    /// Entity the intent appends temporal history for
    EntityId,
    /// Ordinal base the intent's entries start at, part of the dedup key
    Base,
    /// Serialised temporal append intent
    Intent,
    /// Earliest instant the intent may next be claimed
    AvailableAt,
    /// Instant the current lease expires
    LeaseUntil,
    /// Worker that holds the current lease
    LeaseOwner,
    /// Number of claim attempts already made
    Attempts,
    /// Diagnostic recorded by the last failed attempt
    LastError,
}

#[cfg(test)]
mod tests {
    use crate::queue::{PendingSideEffects, TemporalAppendIntents, WriteRecords};
    use sea_query::Iden;

    #[test]
    fn temporal_append_intents_table_and_columns() {
        assert_eq!(TemporalAppendIntents::Table.to_string(), "temporal_append_intents");
        assert_eq!(TemporalAppendIntents::Sequence.to_string(), "sequence");
        assert_eq!(TemporalAppendIntents::WriteId.to_string(), "write_id");
        assert_eq!(TemporalAppendIntents::EntityId.to_string(), "entity_id");
        assert_eq!(TemporalAppendIntents::Base.to_string(), "base");
        assert_eq!(TemporalAppendIntents::Intent.to_string(), "intent");
        assert_eq!(TemporalAppendIntents::AvailableAt.to_string(), "available_at");
        assert_eq!(TemporalAppendIntents::LeaseUntil.to_string(), "lease_until");
        assert_eq!(TemporalAppendIntents::LeaseOwner.to_string(), "lease_owner");
        assert_eq!(TemporalAppendIntents::Attempts.to_string(), "attempts");
        assert_eq!(TemporalAppendIntents::LastError.to_string(), "last_error");
    }

    #[test]
    fn write_records_table_and_columns() {
        assert_eq!(WriteRecords::Table.to_string(), "write_records");
        assert_eq!(WriteRecords::WriteId.to_string(), "write_id");
        assert_eq!(WriteRecords::RecordedAt.to_string(), "recorded_at");
        assert_eq!(WriteRecords::AcceptedAt.to_string(), "accepted_at");
    }

    #[test]
    fn pending_side_effects_table_and_columns() {
        assert_eq!(PendingSideEffects::Table.to_string(), "pending_side_effects");
        assert_eq!(PendingSideEffects::Id.to_string(), "id");
        assert_eq!(PendingSideEffects::WriteId.to_string(), "write_id");
        assert_eq!(PendingSideEffects::RecordedAt.to_string(), "recorded_at");
        assert_eq!(PendingSideEffects::ConsequenceBase.to_string(), "consequence_base");
        assert_eq!(PendingSideEffects::Payload.to_string(), "payload");
        assert_eq!(PendingSideEffects::Attempts.to_string(), "attempts");
        assert_eq!(PendingSideEffects::Dead.to_string(), "dead");
        assert_eq!(PendingSideEffects::LastError.to_string(), "last_error");
        assert_eq!(PendingSideEffects::NextAttemptAt.to_string(), "next_attempt_at");
        assert_eq!(PendingSideEffects::LockedUntil.to_string(), "locked_until");
        assert_eq!(PendingSideEffects::LockedBy.to_string(), "locked_by");
    }
}
