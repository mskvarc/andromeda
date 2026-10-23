//! The relations that hold deferred work.
//!
//! `pending_side_effects` is the durable queue
//! of deferred batch jobs a committing write enqueues, drained at-least-once by
//! workers that lease rows under a time-bounded claim.

use sea_query::Iden;

/// The `pending_side_effects` table - the durable deferred-job queue.
#[derive(Iden, Clone, Copy)]
pub enum PendingSideEffects {
    /// Table name: `pending_side_effects`
    Table,
    /// Job surrogate identity, minted by the enqueue
    Id,
    /// Instant the job was enqueued, the dead-letter retention clock
    CreatedAt,
    /// Identity of the write that enqueued the job, the deduplication key
    WriteId,
    /// Instant the enqueueing write stamped, reused when the job resumes
    RecordedAt,
    /// Ordinal base the enqueueing write's consequences resume from
    BaseOrdinal,
    /// The owed changes, as one `pending_change_event[]`
    Events,
    /// Number of drain attempts already made (shared claimable-queue contract)
    AttemptCount,
    /// Dead-letter door: instant the job was retired, NULL while drainable
    ExhaustedAt,
    /// Diagnostic recorded by the last failed attempt
    LastError,
    /// Earliest instant the job may next be attempted
    NextAttemptAt,
    /// Instant the current lease was acquired
    ClaimedAt,
    /// Instant the current lease expires
    LeaseUntil,
    /// Worker that holds the current lease
    ClaimedBy,
}

/// The `temporal_append_intents` table - the durable temporal-append queue.
#[derive(Iden, Clone, Copy)]
pub enum TemporalAppendIntents {
    /// Table name: `temporal_append_intents`
    Table,
    /// Monotonic queue sequence, the row identity workers order and claim by
    IntentKey,
    /// Identity of the write that produced the intent
    WriteId,
    /// Entity the intent appends temporal history for
    EntityId,
    /// Ordinal base the intent's entries start at, part of the dedup key
    BaseOrdinal,
    /// Ingestion instant of the write, every entry's `recorded_at`
    RecordedAt,
    /// `entity_type_names` keys the Temporal Evolution is recorded under,
    /// empty for an intent that only ends instances
    EntityTypeNameKeys,
    /// The intent's `temporal_entry` values in write order
    Entries,
    /// `TemporalIntentReach` codes of the application statements' needs the entries meet
    EntryNeeds,
    /// Earliest instant the intent may next be claimed (shared claimable-queue contract)
    NextAttemptAt,
    /// Instant the current lease was acquired
    ClaimedAt,
    /// Instant the current lease expires
    LeaseUntil,
    /// Worker that holds the current lease
    ClaimedBy,
    /// Number of claim attempts already made
    AttemptCount,
    /// Diagnostic recorded by the last failed attempt
    LastError,
}

/// The `temporal_append_entries` view: every entry of every queued intent,
/// one row each, beside the write identity its history row will carry.
///
/// Its other columns are the fields of the `temporal_entry` composite, under
/// the names [`TemporalEntry`](crate::temporal_intent::TemporalEntry) gives
/// them.
#[derive(Iden, Clone, Copy)]
pub enum TemporalAppendEntries {
    /// View name: `temporal_append_entries`
    Table,
    /// Key of the intent the entry belongs to
    IntentKey,
    /// Entity the entry records history for
    EntityId,
    /// Ingestion instant of the write
    RecordedAt,
    /// Identity of the write
    WriteId,
    /// The entry's write ordinal: its intent's base ordinal plus its position from 0
    WriteOrdinal,
}

#[cfg(test)]
mod tests {
    use crate::queue::{PendingSideEffects, TemporalAppendEntries, TemporalAppendIntents};
    use pretty_assertions::assert_eq;
    use sea_query::Iden;

    #[test]
    fn temporal_append_intents_table_and_columns() {
        assert_eq!(TemporalAppendIntents::Table.to_string(), "temporal_append_intents");
        assert_eq!(TemporalAppendIntents::IntentKey.to_string(), "intent_key");
        assert_eq!(TemporalAppendIntents::WriteId.to_string(), "write_id");
        assert_eq!(TemporalAppendIntents::EntityId.to_string(), "entity_id");
        assert_eq!(TemporalAppendIntents::BaseOrdinal.to_string(), "base_ordinal");
        assert_eq!(TemporalAppendIntents::RecordedAt.to_string(), "recorded_at");
        assert_eq!(TemporalAppendIntents::EntityTypeNameKeys.to_string(), "entity_type_name_keys");
        assert_eq!(TemporalAppendIntents::Entries.to_string(), "entries");
        assert_eq!(TemporalAppendIntents::NextAttemptAt.to_string(), "next_attempt_at");
        assert_eq!(TemporalAppendIntents::ClaimedAt.to_string(), "claimed_at");
        assert_eq!(TemporalAppendIntents::LeaseUntil.to_string(), "lease_until");
        assert_eq!(TemporalAppendIntents::ClaimedBy.to_string(), "claimed_by");
        assert_eq!(TemporalAppendIntents::AttemptCount.to_string(), "attempt_count");
        assert_eq!(TemporalAppendIntents::LastError.to_string(), "last_error");
        assert_eq!(TemporalAppendIntents::EntryNeeds.to_string(), "entry_needs");
    }

    #[test]
    fn temporal_append_entries_view_and_columns() {
        assert_eq!(TemporalAppendEntries::Table.to_string(), "temporal_append_entries");
        assert_eq!(TemporalAppendEntries::IntentKey.to_string(), "intent_key");
        assert_eq!(TemporalAppendEntries::EntityId.to_string(), "entity_id");
        assert_eq!(TemporalAppendEntries::RecordedAt.to_string(), "recorded_at");
        assert_eq!(TemporalAppendEntries::WriteId.to_string(), "write_id");
        assert_eq!(TemporalAppendEntries::WriteOrdinal.to_string(), "write_ordinal");
    }

    #[test]
    fn pending_side_effects_table_and_columns() {
        assert_eq!(PendingSideEffects::Table.to_string(), "pending_side_effects");
        assert_eq!(PendingSideEffects::Id.to_string(), "id");
        assert_eq!(PendingSideEffects::CreatedAt.to_string(), "created_at");
        assert_eq!(PendingSideEffects::WriteId.to_string(), "write_id");
        assert_eq!(PendingSideEffects::RecordedAt.to_string(), "recorded_at");
        assert_eq!(PendingSideEffects::BaseOrdinal.to_string(), "base_ordinal");
        assert_eq!(PendingSideEffects::Events.to_string(), "events");
        assert_eq!(PendingSideEffects::AttemptCount.to_string(), "attempt_count");
        assert_eq!(PendingSideEffects::ExhaustedAt.to_string(), "exhausted_at");
        assert_eq!(PendingSideEffects::LastError.to_string(), "last_error");
        assert_eq!(PendingSideEffects::NextAttemptAt.to_string(), "next_attempt_at");
        assert_eq!(PendingSideEffects::ClaimedAt.to_string(), "claimed_at");
        assert_eq!(PendingSideEffects::LeaseUntil.to_string(), "lease_until");
        assert_eq!(PendingSideEffects::ClaimedBy.to_string(), "claimed_by");
    }
}
