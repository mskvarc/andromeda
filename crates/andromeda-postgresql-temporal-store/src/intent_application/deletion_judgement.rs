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

use crate::intent_application::intent_source::{I, StagedSource, Te, entry, intent, source_intents_as};
use andromeda_postgresql_codes::temporal_entry_kind::TemporalEntryKind;
use andromeda_postgresql_schema::{queue::TemporalAppendIntents, temporal::TemporalEntities, temporal_intent::TemporalEntry};
use andromeda_postgresql_seaquery::{
    literal::int_literal,
    pg_array::{any, has_elements},
};
use sea_query::{Expr, ExprTrait, Iden, Query, SimpleExpr};

/// The alias of an intent of the source stating types for the Entity of the
/// intent under `i`, `ai`.
#[derive(Iden, Clone, Copy)]
#[iden = "ai"]
struct AnchoringIntent;

/// Whether the entry under `e`, of the intent under `i`, ends an instance of
/// a Temporal Evolution the application created (`created` holds for it)
/// before any intent of the source stating types for its Entity, so it
/// records nothing.
pub(crate) fn ended_before_evolution(source: &StagedSource<'_>, created: SimpleExpr) -> SimpleExpr {
    let mut anchoring = Query::select();
    anchoring.expr(int_literal(1));
    source_intents_as(&mut anchoring, source, AnchoringIntent);
    anchoring
        .and_where(Expr::col((AnchoringIntent, TemporalAppendIntents::EntityId)).equals((I, TemporalAppendIntents::EntityId)))
        .and_where(has_elements(Expr::col((AnchoringIntent, TemporalAppendIntents::EntityTypeNameKeys))))
        .and_where(Expr::col((AnchoringIntent, TemporalAppendIntents::IntentKey)).lte(intent(TemporalAppendIntents::IntentKey)));
    entry(TemporalEntry::EntryKind)
        .eq(TemporalEntryKind::Tombstone.code())
        .and(created)
        .and(Expr::exists(anchoring).not())
}

/// Whether the Temporal Evolution under `te` is one the evolution upsert of
/// the application created, its key among the ones the upsert returned.
pub(crate) fn created_by_application(source: &StagedSource<'_>) -> SimpleExpr {
    Expr::col((Te, TemporalEntities::TemporalEntityKey)).eq(any(source.created_evolutions()))
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
        let staged = StagedSource::new(source, EntryReads::Nothing, created);
        Query::select()
            .column(Asterisk)
            .and_where(ended_before_evolution(&staged, created_by_application(&staged)))
            .to_owned()
            .to_string(PostgresQueryBuilder)
    }

    /// Only the end of an instance waits for its evolution in key order: an
    /// observation of an evolution a later intent creates is recorded, as the
    /// read shows it.
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
                r#""e"."entry_kind" = 2 AND "te"."temporal_entity_key" = ANY(ARRAY [7]::bigint[]) AND (NOT EXISTS(SELECT 1 FROM "#,
                r#"(SELECT * FROM "temporal_append_intents" WHERE "intent_key" = ANY(ARRAY [3,5]::bigint[])) AS "ai" "#,
                r#"WHERE "ai"."entity_id" = "i"."entity_id" AND cardinality("ai"."entity_type_name_keys") > 0 "#,
                r#"AND "ai"."intent_key" <= "i"."intent_key"))"#
            )),
            "{sql}"
        );
    }

    /// A commit's source names the created keys the way the drain's does,
    /// whatever else it binds.
    #[test]
    fn the_created_keys_are_named_the_same_for_a_bound_source() {
        let record = WriteRecord::restored(WriteId::mint(), recorded_at());
        let sql = rendered(&IntentSource::Bound { record: &record, intents: &[] }, &[7, 9]);
        assert!(sql.contains(r#""te"."temporal_entity_key" = ANY(ARRAY [7,9]::bigint[])"#), "{sql}");
    }
}
