//! The candidates of an `idPattern` (clause 5.7.2): the Evolutions whose
//! identifier the pattern matches, read from the catalogue's trigram index.
//!
//! Clause 5.7.4.4 keeps an Entity whose identifier matches the pattern, a test
//! on the identifier alone, and the index rechecks each match against the
//! pattern, so the candidates are exactly the Evolutions the pattern keeps and
//! the narrowed walk does not test it again. The filter tests the `id` column
//! under the collation `idx_entities_id_trgm` declares, and keeps the identity
//! rows holding a Temporal Evolution.

use crate::stage1::queued_candidates::queued_candidates;
use andromeda_ngsi_ld_query::{filter::predicate::PatternPolarity, pattern::Pattern};
use andromeda_postgresql_query::{
    entity_sides::holds_evolution,
    history_source::{HistoryBranch, PendingEntries},
    operator::{PatternIndexes, pattern as pattern_predicate},
};
use andromeda_postgresql_schema::{current::Entities, queue::TemporalAppendIntents};
use andromeda_postgresql_seaquery::branch::qualified;
use sea_query::{Expr, Query, SelectStatement, UnionType};

/// The Evolutions of `branch` whose identifier `pattern` matches: their
/// surrogate keys over recorded rows alone; where the branch reads its queue,
/// their identifiers beside those of the queued Entities the pattern matches.
pub(super) fn pattern_candidates(branch: &HistoryBranch, pattern: &Pattern) -> SelectStatement {
    let column = match branch.pending() {
        PendingEntries::Absent => Entities::EntityKey,
        PendingEntries::Unioned => Entities::Id,
    };
    let mut recorded = Query::select();
    recorded
        .column((Entities::Table, column))
        .from(qualified(branch.schema(), Entities::Table))
        .and_where(pattern_predicate(
            Expr::col((Entities::Table, Entities::Id)),
            PatternPolarity::Match,
            pattern,
            PatternIndexes::TrigramsAndByteOrder,
        ))
        .and_where(holds_evolution(Entities::Table));
    match branch.pending() {
        PendingEntries::Absent => {}
        PendingEntries::Unioned => {
            let mut queued = queued_candidates(branch);
            queued.and_where(pattern_predicate(
                Expr::col(TemporalAppendIntents::EntityId),
                PatternPolarity::Match,
                pattern,
                PatternIndexes::None,
            ));
            recorded.union(UnionType::All, queued);
        }
    }
    recorded.take()
}

#[cfg(test)]
mod tests {
    use crate::stage1::{
        Stage1Query,
        query_fixture::{id_pattern, opened, recorded_stage1, stage1_on},
    };
    use andromeda_ngsi_ld_query::{
        entity_selection::EntitySelector,
        ordering::{collation::IcuCollation, sort_direction::SortDirection},
        page_window::PageWindow,
        pattern::Pattern,
        temporal::ordering::TemporalEntityOrdering,
    };
    use andromeda_postgresql_query::history_source::HistoryBranch;
    use andromeda_postgresql_seaquery::{assert_statement_snapshot, statement_planning::StatementPlanning};
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, IntoIden};

    /// The catalogue query the step tests vary: `idPattern=.*:0123.*`, no
    /// type.
    fn catalogue() -> Stage1Query {
        recorded_stage1(&id_pattern(None, ".*:0123.*"))
    }

    #[test]
    fn an_unanchored_pattern_over_the_catalogue_reads_the_identifier_trigrams() {
        let stage1 = catalogue();
        assert_eq!(stage1.planning, StatementPlanning::GenericGatedBranches);
        let statement = stage1.select;
        assert_statement_snapshot!(
            "pattern-candidates-an-unanchored-pattern-over-the-catalogue-reads-the-identifier-trigrams",
            statement
        );
    }

    #[test]
    fn the_pattern_test_is_the_expression_the_trigram_index_holds() {
        let statement = catalogue().select;
        assert_statement_snapshot!("pattern-candidates-the-pattern-test-is-the-expression-the-trigram-index-holds", statement);
    }

    /// The identifier index answers an anchored pattern's start, so the query
    /// takes the step of the window clause 5.7.4.4 makes every query carry,
    /// and the pattern stays in the probe.
    #[test]
    fn an_anchored_pattern_reads_no_trigrams() {
        let prefix = recorded_stage1(&opened(id_pattern(None, "^urn:ngsi-ld:Sensor:0123.*"))).select;
        assert_statement_snapshot!("pattern-candidates-an-anchored-pattern-reads-no-trigrams-1", prefix);
        let exact = recorded_stage1(&opened(id_pattern(None, "^urn:ngsi-ld:Sensor:012345$"))).select;
        assert_statement_snapshot!("pattern-candidates-an-anchored-pattern-reads-no-trigrams-2", exact);
    }

    #[test]
    fn a_typed_selector_narrows_the_type_walk_by_the_trigram_candidates() {
        let statement = recorded_stage1(&id_pattern(Some("Sensor"), ".*:0123.*")).select;
        assert_statement_snapshot!("pattern-candidates-a-typed-selector-narrows-the-type-walk-by-the-trigram-candidates", statement);
    }

    #[test]
    fn an_attrs_selection_narrows_the_holder_walk_by_the_trigram_candidates() {
        let mut compiled = id_pattern(None, ".*:0123.*");
        compiled.instances.attr_filter = vec![IriBuf::new("https://uri.etsi.org/ngsi-ld/default-context/temperature".to_owned()).expect("valid IRI")];
        let statement = recorded_stage1(&compiled).select;
        assert_statement_snapshot!(
            "pattern-candidates-an-attrs-selection-narrows-the-holder-walk-by-the-trigram-candidates",
            statement
        );
    }

    /// Paired selectors read no trigrams and take the window's step; a
    /// requested ordering takes no step at all.
    #[test]
    fn paired_selectors_read_no_trigrams_and_an_ordering_keeps_the_plain_walk() {
        let mut paired = opened(id_pattern(Some("Sensor"), ".*:0123.*"));
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
        let paired = recorded_stage1(&paired).select;
        assert_statement_snapshot!(
            "pattern-candidates-paired-selectors-read-no-trigrams-and-an-ordering-keeps-the-plain-walk-1",
            paired
        );
        let stage1 = recorded_stage1(&ordered);
        assert_eq!(stage1.planning, StatementPlanning::Cached);
        let ordered = stage1.select;
        assert_statement_snapshot!(
            "pattern-candidates-paired-selectors-read-no-trigrams-and-an-ordering-keeps-the-plain-walk-2",
            ordered
        );
    }

    #[test]
    fn a_queued_branch_compares_identifiers_and_admits_queued_entities() {
        let statement = stage1_on(&HistoryBranch::live(), &id_pattern(None, ".*:0123.*"), &PageWindow::offset(20, 0)).select;
        assert_statement_snapshot!("pattern-candidates-a-queued-branch-compares-identifiers-and-admits-queued-entities", statement);
    }

    #[test]
    fn a_snapshot_reads_its_own_trigram_index() {
        let statement = stage1_on(
            &HistoryBranch::recorded_in(Alias::new("snap_7").into_iden()),
            &id_pattern(None, ".*:0123.*"),
            &PageWindow::offset(20, 0),
        )
        .select;
        assert_statement_snapshot!("pattern-candidates-a-snapshot-reads-its-own-trigram-index", statement);
    }

    #[test]
    fn a_counted_pattern_counts_under_the_count_bound() {
        let stage1 = catalogue();
        assert_eq!(stage1.count_planning, StatementPlanning::BoundValues);
        let statement = stage1.count.expect("count requested");
        assert_statement_snapshot!("pattern-candidates-a-counted-pattern-counts-under-the-count-bound", statement);
    }
}
