//! The page of a type selection filtered by an `idPattern` alone (clause
//! 5.7.2), when the pattern leaves the start of the identifier open.
//!
//! The type's membership relation holds its members in identifier order with
//! no index of the identifier's pieces, so the plain page tests the pattern
//! on every member before the first match past a rare one. A pattern that
//! fixes the identifier's start is a range of that order and stays on the
//! plain page. Otherwise the trigram index over `entities.id` yields the
//! Entities of every type whose identifier matches, each rechecked against
//! the pattern, and the type keeps its members among them. The planner cannot
//! price the pattern, so the page is a [`gated_page`]:
//!
//! 1. the short walk tests the first [`ID_PATTERN_SHORT_WALK`] members in
//!    identifier order;
//! 2. the index reads at most [`ID_PATTERN_CANDIDATE_BOUND`] + 1 matches, and
//!    the page is cut from the members among them;
//! 3. the full walk tests every member.

use crate::{
    entity_page::{ProjectedPage, sort_column},
    entity_relations::{EntityRelations, RowSource},
    gated_page::{GatedColumns, IndexRead, PageSource, PageWalk, Walked, gated_page},
};
use andromeda_ngsi_ld_query::{
    entity::compiled::CompiledEntityQuery,
    entity_selection::ConsideredEntities,
    pattern::Pattern,
    projection::projected_members::ProjectedMembers,
    read_order::ReadOrder,
};
use andromeda_postgresql_query::{
    entity_row_refs::EntityRowRefs,
    error::RenderError,
    id_pattern_trigrams::{ID_PATTERN_CANDIDATE_BOUND, ID_PATTERN_SHORT_WALK, trigram_pattern},
    one_time_gate::CandidateBound,
    operator::{PatternIndexes, pattern as pattern_predicate},
    standing_instant::instant_of,
};
use andromeda_postgresql_schema::current::Entities;
use andromeda_postgresql_stored_form::entity_sides::holds_standing_state;
use sea_query::{Condition, DynIden, Expr, Query, SelectStatement};

/// An `idPattern` and the Entities of any type whose identifier it matches.
pub(crate) struct PatternMatches<'q> {
    /// The selector's pattern.
    pub(crate) pattern: &'q Pattern,
    /// `SELECT "entity_key" FROM "entities" WHERE ("id") COLLATE "default" ~ <pattern> AND
    /// "created_at" IS NOT NULL AND ("expires_at" IS NULL OR "expires_at" > <instant>)`, the
    /// instant the query's selection stands at.
    pub(crate) select: SelectStatement,
}

/// The matches of the query's `idPattern`, when the statement walks the
/// members of one selector's type in identifier order, that selector's
/// pattern leaves the identifier's start open, and nothing else filters.
pub(crate) fn pattern_matches<'q>(query: &'q CompiledEntityQuery, relations: &EntityRelations, schema: Option<&DynIden>) -> Option<PatternMatches<'q>> {
    let selection = &query.selection;
    let walked = match relations.source {
        RowSource::TypeMembers { .. } => true,
        RowSource::Entities => false,
    };
    let alone = match (selection.considered, &selection.projected_members) {
        (ConsideredEntities::Selected, ProjectedMembers::Unconditional) => {
            selection.attrs.is_empty() && selection.q.is_none() && selection.geo.is_none() && selection.scope_q.is_none() && query.ordering.terms.is_empty()
        }
        (ConsideredEntities::NoEntity, _) | (ConsideredEntities::Selected, ProjectedMembers::AnyOf { .. } | ProjectedMembers::AnyBesides { .. }) => false,
    };
    let [selector] = selection.selectors.as_slice() else {
        return None;
    };
    let pattern = trigram_pattern(selector.identifier_axis())?;
    if !(walked && alone) {
        return None;
    }
    let rows = EntityRowRefs::unaliased(schema.cloned()).standing_at(selection.standing);
    let mut matches = Query::select();
    matches
        .expr(rows.entity_key_column())
        .from(rows.table(Entities::Table))
        .and_where(pattern_predicate(
            rows.identity_column(Entities::Id),
            pattern,
            PatternIndexes::TrigramsAndByteOrder,
        ))
        .and_where(holds_standing_state(Entities::Table, instant_of(rows.standing())));
    Some(PatternMatches {
        pattern,
        select: matches.take(),
    })
}

/// The projected page of `source`, a type selection filtered by an
/// `idPattern` alone, cut from the pattern's `matches`, and the order its
/// rows come back in, its rows carrying what `columns` asks for.
///
/// # Errors
///
/// Fails as rendering the plain page fails.
pub(crate) fn id_pattern_page(source: &PageSource<'_>, matches: PatternMatches<'_>, columns: GatedColumns) -> Result<(ProjectedPage, ReadOrder), RenderError> {
    // Ordered by nothing but the identifier, the walk projects it as its
    // first sort key.
    let walked_identifier = Expr::col((Walked, sort_column(0)));
    let walk = PageWalk {
        source: *source,
        walked: Condition::all(),
        probe: Condition::all().add(pattern_predicate(walked_identifier, matches.pattern, PatternIndexes::None)),
        short: ID_PATTERN_SHORT_WALK,
    };
    let read = IndexRead {
        candidates: matches.select,
        bound: CandidateBound::AtMost(ID_PATTERN_CANDIDATE_BOUND),
        // The pattern is the one filter, and every match the index yields is
        // rechecked against it.
        narrowed: Condition::all(),
    };
    gated_page(source, walk, vec![read], columns)
}

#[cfg(test)]
mod tests {
    use crate::{
        entity::{EntityQuery, build_query},
        entity_total::EntityTotal,
    };
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_query::{
        count_policy::CountPolicy,
        entity::compiled::CompiledEntityQuery,
        entity_selection::{EntitySelection, EntitySelector},
        filter::{compiled_filter::CompiledFilter, language_matching::LanguageMatching, parser::parse},
        ordering::term::{CompiledOrderTerm, OrderDirection, OrderTarget},
        page_window::PageWindow,
        pattern::Pattern,
        type_selection::grammar::TypeExpr,
    };
    use andromeda_postgresql_query::query_names::QueryNames;
    use andromeda_postgresql_seaquery::{assert_statement_snapshot, statement_planning::StatementPlanning};
    use andromeda_postgresql_stored_values::{attribute_name_key::AttributeNameKey, entity_type_name_key::EntityTypeNameKey, resolved_names::ResolvedNames};
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;
    use sea_query::SelectStatement;

    fn names() -> QueryNames {
        QueryNames {
            attributes: ResolvedNames::from_pairs([("https://example.com/headSign".parse().expect("valid name"), AttributeNameKey::from(28))]),
            types: ResolvedNames::from_pairs([("StopTime", 18), ("Trip", 3)].map(|(name, key)| {
                (
                    format!("https://example.com/{name}").parse().expect("valid type name"),
                    EntityTypeNameKey::from(key),
                )
            })),
            ..QueryNames::default()
        }
    }

    fn selector(ty: Option<&str>, pattern: &str) -> EntitySelector {
        EntitySelector::builder()
            .type_expr(ty.map(|ty| TypeExpr::Type(IriBuf::new(format!("https://example.com/{ty}")).expect("valid IRI"))))
            .id_pattern(Some(Pattern::parse(pattern).expect("valid pattern")))
            .build()
    }

    fn by_pattern(pattern: &str) -> CompiledEntityQuery {
        let mut query = CompiledEntityQuery::builder().selection(EntitySelection::builder().build()).build();
        query.selection.selectors = vec![selector(Some("StopTime"), pattern)];
        query
    }

    fn built(query: &CompiledEntityQuery, count: CountPolicy) -> EntityQuery {
        build_query(&ContextTarget::Live, query, &PageWindow::offset(100, 0), count, &names()).expect("valid entity query")
    }

    fn page_select(query: &CompiledEntityQuery) -> SelectStatement {
        built(query, CountPolicy::Omit).select
    }

    #[test]
    fn an_unanchored_id_pattern_beside_a_type_reads_the_identifier_trigrams() {
        let built = built(&by_pattern(".*_1196287$"), CountPolicy::Omit);
        assert_eq!(built.planning, StatementPlanning::GatedBranches);
        assert_statement_snapshot!(
            "id-pattern-page-an-unanchored-id-pattern-beside-a-type-reads-the-identifier-trigrams",
            built.select
        );
    }

    #[test]
    fn the_short_walk_tests_the_members_and_the_full_walk_keeps_the_pattern() {
        assert_statement_snapshot!(
            "id-pattern-page-the-short-walk-tests-the-members-and-the-full-walk-keeps-the-pattern",
            page_select(&by_pattern(".*_1196287$"))
        );
    }

    #[test]
    fn an_anchored_id_pattern_keeps_the_type_range() {
        for (index, pattern) in ["^urn:ngsi-ld:StopTime:99.*", "^urn:ngsi-ld:StopTime:1012_03_478_1196287$"]
            .into_iter()
            .enumerate()
        {
            let built = built(&by_pattern(pattern), CountPolicy::Omit);
            assert_statement_snapshot!(
                format!("id-pattern-page-an-anchored-id-pattern-keeps-the-type-range-{}", index + 1),
                built.select
            );
            assert_eq!(built.planning, StatementPlanning::Cached, "{pattern}");
        }
    }

    /// Driven by `entities`, the plain page can read the trigram index
    /// itself.
    #[test]
    fn an_id_pattern_without_a_type_keeps_the_plain_page() {
        let mut query = by_pattern(".*_1196287$");
        query.selection.selectors = vec![selector(None, ".*_1196287$")];
        assert_statement_snapshot!("id-pattern-page-an-id-pattern-without-a-type-keeps-the-plain-page", page_select(&query));
    }

    #[test]
    fn an_id_pattern_filtered_further_or_ordered_keeps_the_plain_page() {
        let mut filtered = by_pattern(".*_1196287$");
        filtered.selection.q = Some(CompiledFilter::new(
            parse("\"https://example.com/headSign\"!=1").expect("valid q"),
            LanguageMatching::TagEquality,
        ));
        let mut by_attrs = by_pattern(".*_1196287$");
        by_attrs.selection.attrs = vec![IriBuf::new("https://example.com/headSign".to_string()).expect("valid IRI")];
        let mut ordered = by_pattern(".*_1196287$");
        ordered.ordering.terms = vec![CompiledOrderTerm {
            target: OrderTarget::ModifiedAt,
            direction: OrderDirection::Desc,
            collation: None,
        }];
        let mut paired = by_pattern(".*_1196287$");
        paired.selection.selectors.push(selector(Some("Trip"), ".*_1$"));
        for (index, query) in [filtered, by_attrs, ordered, paired].into_iter().enumerate() {
            let built = built(&query, CountPolicy::Omit);
            assert_statement_snapshot!(
                format!("id-pattern-page-an-id-pattern-filtered-further-or-ordered-keeps-the-plain-page-{}", index + 1),
                built.select
            );
            assert_eq!(built.planning, StatementPlanning::Cached);
        }
    }

    /// Clause 6.3.13: the matches branch carries the total on its rows; when
    /// another branch answers, the total is its own count, read from the
    /// matches while they are few, or walked at once when the short walk
    /// filled the page.
    #[test]
    fn a_counted_id_pattern_page_counts_the_members_among_the_matches_under_the_rechecked_bound() {
        let built = built(&by_pattern(".*_1196287$"), CountPolicy::Emit);
        let EntityTotal::OnPageRows { uncounted } = built.total else {
            panic!("a first gated page carries its total when the matches answer");
        };
        assert_statement_snapshot!(
            "id-pattern-page-a-counted-id-pattern-page-counts-the-members-among-the-matches-under-the-rechecked-bound-1",
            built.select
        );
        let walked_at_once = uncounted.walked.expect("a page the short walk filled is counted by walking").select;
        assert_statement_snapshot!(
            "id-pattern-page-a-counted-id-pattern-page-counts-the-members-among-the-matches-under-the-rechecked-bound-2",
            walked_at_once
        );
        assert_statement_snapshot!(
            "id-pattern-page-a-counted-id-pattern-page-counts-the-members-among-the-matches-under-the-rechecked-bound-3",
            uncounted.count
        );
        assert_eq!(built.total_planning, StatementPlanning::BoundValues);
    }
}
