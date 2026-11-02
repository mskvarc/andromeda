//! Which ends of an instance an application records.
//!
//! The end of an instance a Core API write deleted is recorded only in a
//! Temporal Evolution that exists when its intent is applied: clause 5.6.16
//! removes an evolution entirely, and clause 4.5.6 makes the evolution the
//! Entity of clause 4.5.1, which only an intent stating types brings into
//! being. Applied on its own, an intent finds the evolution its Entity held
//! before the application, or the one an intent of the source stating types
//! at or before its own created. The statements of an application run over
//! every intent of the source at once, so a statement after the evolution
//! upsert also sees an evolution a later intent of the Entity created. The
//! upsert therefore returns the keys of the evolutions it created, and an end
//! of an instance of one of them is dropped unless an intent stating types
//! comes at or before its own. A temporal read of the queue judges a queued
//! end of an instance by the same rule, so the history a temporal query
//! answers with (clause 5.7.4) is the same before the drain and after it,
//! however the drain groups the intents.
//!
//! The first intent stating types is read from the column the source carries
//! beside each intent, computed once for the whole source: a search of the
//! source for each entry costs the square of the source's size, and a
//! generic plan, which estimates the source at a handful of rows, cannot see
//! that cost.

use crate::intent_application::intent_source::{FirstTypedIntent, I, StagedSource, entry, intent};
use andromeda_postgresql_codes::temporal_entry_kind::TemporalEntryKind;
use andromeda_postgresql_schema::{current::Entities, queue::TemporalAppendIntents, temporal_intent::TemporalEntry};
use andromeda_postgresql_temporal_query::alias::Te;
use sea_query::{Expr, ExprTrait, SimpleExpr};

/// Whether the entry under `e`, of the intent under `i`, ends an instance of
/// a Temporal Evolution the application created (`created` holds for it)
/// before any intent of the source stating types for its Entity, so it
/// records nothing.
pub(crate) fn ended_before_evolution(created: SimpleExpr) -> SimpleExpr {
    let first_typed = || Expr::col((I, FirstTypedIntent));
    entry(TemporalEntry::EntryKind)
        .eq(TemporalEntryKind::Tombstone.code())
        .and(created)
        .and(first_typed().is_null().or(first_typed().gt(intent(TemporalAppendIntents::IntentKey))))
}

/// Whether the Temporal Evolution under `te` is one the evolution upsert of
/// the application created, its key among the ones the upsert returned.
pub(crate) fn created_by_application(source: &StagedSource<'_>) -> SimpleExpr {
    source.is_created_evolution(Expr::col((Te, Entities::EntityKey)))
}

#[cfg(test)]
mod tests {
    use crate::{
        intent_application::{
            deletion_judgement::{created_by_application, ended_before_evolution},
            intent_source::{EntryReads, IntentSource, StagedSource},
        },
        temporal_entry_record::tests::recorded_at,
    };
    use andromeda_context_commit::{write_id::WriteId, write_record::WriteRecord};
    use andromeda_postgresql_codes::temporal_intent_reach::TemporalIntentReach;
    use sea_query::{Asterisk, PostgresQueryBuilder, Query};

    fn rendered(source: &IntentSource<'_>, created: &[i64]) -> String {
        let staged = StagedSource::new(source, EntryReads::Identities, created);
        Query::select()
            .column(Asterisk)
            .and_where(ended_before_evolution(created_by_application(&staged)))
            .to_owned()
            .to_string(PostgresQueryBuilder)
    }

    /// Only the end of an instance waits for its evolution in key order: an
    /// observation of an evolution a later intent creates is recorded, as the
    /// read shows it. The order is read from the intent's own row, never by
    /// searching the source again.
    #[test]
    fn only_the_end_of_an_instance_before_the_first_typed_intent_of_a_created_evolution_is_dropped() {
        let sql = rendered(
            &IntentSource::Queued {
                keys: &[3, 5],
                reach: TemporalIntentReach::ALL.to_vec(),
            },
            &[7],
        );
        assert!(
            sql.contains(concat!(
                r#""e"."entry_kind" = 2 AND "te"."entity_key" = 7 AND "#,
                r#"("i"."first_typed_intent" IS NULL OR "i"."first_typed_intent" > "i"."intent_key")"#
            )),
            "{sql}"
        );
        assert!(!sql.contains("EXISTS") && !sql.contains("temporal_append_intents"), "{sql}");
    }

    /// A commit's source names the created keys the way the drain's does,
    /// whatever else it binds.
    #[test]
    fn the_created_keys_are_named_the_same_for_a_bound_source() {
        let record = WriteRecord::restored(WriteId::mint(), recorded_at());
        let sql = rendered(
            &IntentSource::Bound {
                record: &record,
                intents: &[],
                established: &[],
            },
            &[7, 9],
        );
        assert!(sql.contains(r#""te"."entity_key" = ANY(ARRAY [7,9])"#), "{sql}");
    }
}
