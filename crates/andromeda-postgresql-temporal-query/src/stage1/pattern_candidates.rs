//! The candidates of an `idPattern` (clause 5.7.2): the Evolutions whose
//! identifier the pattern matches, read from the catalogue's trigram index.
//!
//! Clause 5.7.4.4 keeps an Entity whose identifier matches the pattern, a test
//! on the identifier alone, and the index rechecks each match against the
//! pattern, so the candidates are exactly the Evolutions the pattern keeps and
//! the narrowed walk does not test it again. The filter is written on the
//! bare `id` column, the expression `idx_entities_id_trgm` holds, and keeps
//! the identity rows holding a Temporal Evolution.

use crate::stage1::queued_candidates::queued_candidates;
use andromeda_ngsi_ld_query::pattern::Pattern;
use andromeda_postgresql_query::{
    entity_sides::holds_evolution,
    history_source::{HistoryBranch, PendingEntries},
};
use andromeda_postgresql_schema::{current::Entities, queue::TemporalAppendIntents};
use andromeda_postgresql_seaquery::{branch::qualified, operator::regex_match};
use sea_query::{Expr, Query, SelectStatement, UnionType};

/// The Evolutions of `branch` whose identifier `pattern` matches: their
/// surrogate keys over recorded rows alone; where the branch reads its queue,
/// their identifiers beside those of the queued Entities the pattern matches.
pub(super) fn pattern_candidates(branch: &HistoryBranch, pattern: &Pattern) -> SelectStatement {
    let matched = || Expr::val(pattern.portable().as_str());
    let column = match branch.pending() {
        PendingEntries::Absent => Entities::EntityKey,
        PendingEntries::Unioned => Entities::Id,
    };
    let mut recorded = Query::select();
    recorded
        .column((Entities::Table, column))
        .from(qualified(branch.schema(), Entities::Table))
        .and_where(regex_match(Expr::col((Entities::Table, Entities::Id)), matched()))
        .and_where(holds_evolution(Entities::Table));
    match branch.pending() {
        PendingEntries::Absent => {}
        PendingEntries::Unioned => {
            let mut queued = queued_candidates(branch);
            queued.and_where(regex_match(Expr::col(TemporalAppendIntents::EntityId), matched()));
            recorded.union(UnionType::All, queued);
        }
    }
    recorded.take()
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::stage1::{
        Stage1Query,
        query_fixture::{CountParts, PageBranches, id_pattern, recorded_stage1, stage1_on},
    };
    use andromeda_ngsi_ld_query::{
        entity_selection::EntitySelector,
        ordering::{collation::IcuCollation, sort_direction::SortDirection},
        page_window::PageWindow,
        pattern::Pattern,
        temporal::ordering::TemporalEntityOrdering,
    };
    use andromeda_postgresql_query::history_source::HistoryBranch;
    use andromeda_postgresql_seaquery::statement_planning::StatementPlanning;
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, IntoIden, PostgresQueryBuilder, Value};

    /// The catalogue query the step tests vary: `idPattern=.*:0123.*`, no
    /// type.
    fn catalogue() -> Stage1Query {
        recorded_stage1(&id_pattern(None, ".*:0123.*"))
    }

    #[test]
    fn an_unanchored_pattern_over_the_catalogue_reads_the_identifier_trigrams() {
        let stage1 = catalogue();
        assert_eq!(stage1.planning, StatementPlanning::GatedBranches);
        let (sql, values) = stage1.select.build(PostgresQueryBuilder);
        assert!(
            sql.contains(r#""candidates" AS  MATERIALIZED (SELECT "entities"."entity_key" FROM "entities" WHERE ("entities"."id" ~ $"#),
            "{sql}"
        );
        for bound in [5_000, 1_001, 1_000] {
            assert!(values.0.contains(&Value::BigUnsigned(Some(bound))), "{bound}: {:?}", values.0);
        }
    }

    #[test]
    fn the_pattern_test_is_the_expression_the_trigram_index_holds() {
        let sql = catalogue().select.to_string(PostgresQueryBuilder);
        let candidates = PageBranches::of(&sql).candidates;
        assert_eq!(
            candidates,
            r#"SELECT "entities"."entity_key" FROM "entities" WHERE ("entities"."id" ~ '.*:0123.*') AND "entities"."evolution_created_at" IS NOT NULL"#
        );
        assert!(!candidates.contains("lower(") && !candidates.contains("::"), "{candidates}");
    }

    /// The identifier index answers an anchored pattern's start, so the query
    /// takes the step of the window clause 5.7.4.4 makes every query carry,
    /// and the pattern stays in the probe.
    #[test]
    fn an_anchored_pattern_reads_no_trigrams() {
        for raw in ["^urn:ngsi-ld:Sensor:0123.*", "^urn:ngsi-ld:Sensor:012345$"] {
            let sql = recorded_stage1(&id_pattern(None, raw)).select.to_string(PostgresQueryBuilder);
            let branches = PageBranches::of(&sql);
            assert!(!branches.candidates.contains(r#""entities"."id" ~"#), "{raw}: {sql}");
            assert!(branches.indexed.contains(r#""e"."id" ~ '^urn:ngsi-ld:Sensor:012"#), "{raw}: {sql}");
        }
    }

    #[test]
    fn a_typed_selector_narrows_the_type_walk_by_the_trigram_candidates() {
        let sql = recorded_stage1(&id_pattern(Some("Sensor"), ".*:0123.*")).select.to_string(PostgresQueryBuilder);
        let indexed = PageBranches::of(&sql).indexed;
        assert!(
            indexed.contains(concat!(
                r#"FROM "entity_type_members" WHERE "entity_type_name_key" = (1) AND "is_evolution_member" AND "entity_key" IN (SELECT "entity_key" FROM "candidates")"#,
                r#" ORDER BY "id" ASC OFFSET 0) AS "e""#
            )),
            "{indexed}"
        );
    }

    #[test]
    fn an_attrs_selection_narrows_the_holder_walk_by_the_trigram_candidates() {
        let mut compiled = id_pattern(None, ".*:0123.*");
        compiled.instances.attr_filter = vec![IriBuf::new("https://uri.etsi.org/ngsi-ld/default-context/temperature".to_owned()).expect("valid IRI")];
        let sql = recorded_stage1(&compiled).select.to_string(PostgresQueryBuilder);
        let indexed = PageBranches::of(&sql).indexed;
        assert!(
            indexed.contains(concat!(
                r#"WHERE "hs"."attribute_name_key" = (1) AND "hs"."entity_key" IN (SELECT "entity_key" FROM "candidates")"#,
                r#" ORDER BY "hi"."id" ASC) AS "hr""#
            )),
            "{indexed}"
        );
    }

    /// Paired selectors read no trigrams and take the window's step; a
    /// requested ordering takes no step at all.
    #[test]
    fn paired_selectors_read_no_trigrams_and_an_ordering_keeps_the_plain_walk() {
        let mut paired = id_pattern(Some("Sensor"), ".*:0123.*");
        paired.entities.selectors.push(
            EntitySelector::builder()
                .id_pattern(Some(Pattern::parse(".*:9.*").expect("valid pattern")))
                .build(),
        );
        let mut ordered = id_pattern(None, ".*:0123.*");
        ordered.ordering = Some(TemporalEntityOrdering {
            direction: SortDirection::Ascending,
            collation: Some(IcuCollation::parse("de-u-co-phonebk").expect("valid collation")),
        });
        let sql = recorded_stage1(&paired).select.to_string(PostgresQueryBuilder);
        let branches = PageBranches::of(&sql);
        assert!(!branches.candidates.contains(r#""entities"."id" ~"#), "{sql}");
        assert!(branches.indexed.contains(r#""e"."id" ~ '.*:9.*'"#), "{sql}");
        let stage1 = recorded_stage1(&ordered);
        let sql = stage1.select.to_string(PostgresQueryBuilder);
        assert!(!sql.contains("candidates"), "{sql}");
        assert_eq!(stage1.planning, StatementPlanning::Cached);
    }

    #[test]
    fn a_queued_branch_compares_identifiers_and_admits_queued_entities() {
        let sql = stage1_on(&HistoryBranch::live(), &id_pattern(None, ".*:0123.*"), &PageWindow::offset(20, 0))
            .select
            .to_string(PostgresQueryBuilder);
        let branches = PageBranches::of(&sql);
        assert!(
            branches.candidates.starts_with(
                r#"SELECT "entities"."id" FROM "entities" WHERE ("entities"."id" ~ '.*:0123.*') AND "entities"."evolution_created_at" IS NOT NULL UNION ALL "#
            ),
            "{}",
            branches.candidates
        );
        assert!(
            branches
                .candidates
                .contains(r#"(SELECT "entity_id" FROM "temporal_append_intents" WHERE "entity_id" ~ '.*:0123.*')"#),
            "{}",
            branches.candidates
        );
        assert!(
            branches.indexed.contains(r#""e"."id" IN (SELECT "id" FROM "candidates")"#),
            "{}",
            branches.indexed
        );
    }

    #[test]
    fn a_snapshot_reads_its_own_trigram_index() {
        let sql = stage1_on(
            &HistoryBranch::recorded_in(Alias::new("snap_7").into_iden()),
            &id_pattern(None, ".*:0123.*"),
            &PageWindow::offset(20, 0),
        )
        .select
        .to_string(PostgresQueryBuilder);
        assert!(PageBranches::of(&sql).candidates.contains(r#"FROM "snap_7"."entities" WHERE"#), "{sql}");
    }

    #[test]
    fn a_counted_pattern_counts_under_the_count_bound() {
        let stage1 = catalogue();
        assert_eq!(stage1.count_planning, StatementPlanning::BoundValues);
        let count = stage1.count.expect("count requested");
        let (sql, values) = count.build(PostgresQueryBuilder);
        assert!(sql.starts_with(r#"WITH "candidates" AS  MATERIALIZED ("#), "{sql}");
        assert!(values.0.contains(&Value::BigUnsigned(Some(10_001))), "{:?}", values.0);
        assert!(values.0.contains(&Value::BigUnsigned(Some(10_000))), "{:?}", values.0);
        let parts = CountParts::of(&sql);
        assert!(
            parts.indexed.contains(r#""e"."entity_key" IN (SELECT "entity_key" FROM "candidates")"#),
            "{}",
            parts.indexed
        );
        assert_eq!(parts.walked.matches("candidates").count(), 2, "the two gates alone: {}", parts.walked);
        assert!(
            parts.filtered.contains(r#""e"."id" ~ $"#),
            "the pattern is tested once, on every looked-up Evolution: {}",
            parts.filtered
        );
        assert!(!parts.indexed.contains(r#""e"."id" ~"#) && !parts.walked.contains(r#""e"."id" ~"#), "{sql}");
        assert!(!sql.contains("CASE WHEN"), "{sql}");
    }
}
