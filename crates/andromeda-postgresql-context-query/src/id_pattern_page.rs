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
    entity::QFilter,
    entity_page::{ProjectedPage, sort_column},
    entity_relations::{EntityRelations, RowSource},
    gated_page::{GatedColumns, IndexOrWalk, PageSource, ShortWalk, Walked, gated_page},
    one_time_gate::CandidateBound,
};
use andromeda_ngsi_ld_query::{
    entity::compiled::CompiledEntityQuery,
    entity_selection::ConsideredEntities,
    identifier_axis::IdentifierAxis,
    pattern::Pattern,
    projection::projected_members::ProjectedMembers,
    read_order::ReadOrder,
    subject_start::SubjectStart,
};
use andromeda_postgresql_query::{entity_row_refs::EntityRowRefs, error::RenderError};
use andromeda_postgresql_schema::current::Entities;
use andromeda_postgresql_seaquery::operator::regex_match;
use sea_query::{Condition, DynIden, Expr, Query, SelectStatement};

/// How many members the short walk tests before the index is read.
///
/// A visit reads one entry of the membership relation's primary key and
/// tests the pattern on the identifier it holds, with no other relation, so
/// the walk is far longer than a keyed page's: a pattern one member in fifty
/// matches fills a page here, and walking it costs a few milliseconds.
pub const ID_PATTERN_SHORT_WALK: u32 = 5_000;

/// The most matches of the trigram index the page reads.
///
/// The index is a GIN, whose matches are gathered in full before the first
/// is returned, so a bound of more matches saves no read; past it, the
/// pattern is dense enough that the walk fills the page sooner than the
/// matches are sorted.
pub const ID_PATTERN_CANDIDATE_BOUND: u64 = 1_000;

/// An `idPattern` and the Entities of any type whose identifier it matches.
pub(crate) struct PatternMatches<'q> {
    /// The selector's pattern.
    pub(crate) pattern: &'q Pattern,
    /// `SELECT "entity_key" FROM "entities" WHERE "id" ~ <pattern>`.
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
            selection.attrs.is_empty()
                && selection.q_expr.is_none()
                && selection.geo.is_none()
                && selection.scope_q.is_none()
                && query.ordering.terms.is_empty()
        }
        (ConsideredEntities::NoEntity, _) | (ConsideredEntities::Selected, ProjectedMembers::AnyOf { .. } | ProjectedMembers::AnyBesides { .. }) => false,
    };
    let [selector] = selection.selectors.as_slice() else {
        return None;
    };
    let IdentifierAxis::Pattern(pattern) = selector.identifier_axis() else {
        return None;
    };
    let open = match pattern.subject_start() {
        SubjectStart::Open => true,
        SubjectStart::Fixed => false,
    };
    if !(walked && alone && open) {
        return None;
    }
    let rows = EntityRowRefs::unaliased(schema.cloned());
    let mut matches = Query::select();
    matches
        .expr(rows.entity_key_column())
        .from(rows.table(Entities::Table))
        .and_where(regex_match(rows.identity_column(Entities::Id), Expr::val(pattern.portable().as_str())));
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
    let short = ShortWalk {
        source: *source,
        walked: Condition::all(),
        probe: Condition::all().add(regex_match(walked_identifier, Expr::val(matches.pattern.portable().as_str()))),
        length: ID_PATTERN_SHORT_WALK,
    };
    let rest = IndexOrWalk {
        candidates: matches.select,
        bound: CandidateBound::AtMost(ID_PATTERN_CANDIDATE_BOUND),
        // The pattern is the one filter, and every match the index yields is
        // rechecked against it.
        narrowed: Condition::all(),
        whole: source.condition(QFilter::Whole)?,
    };
    gated_page(source, short, rest, columns)
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
        filter::parser::parse,
        ordering::term::{CompiledOrderTerm, OrderDirection, OrderTarget},
        page_window::PageWindow,
        pattern::Pattern,
        type_selection::grammar::TypeExpr,
    };
    use andromeda_postgresql_query::{
        attribute_name_key::AttributeNameKey,
        dictionary::{query_names::QueryNames, resolved_names::ResolvedNames},
        entity_type_name_key::EntityTypeNameKey,
    };
    use andromeda_postgresql_seaquery::statement_planning::StatementPlanning;
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, Value};

    fn names() -> QueryNames {
        QueryNames {
            attributes: ResolvedNames::from_pairs([("https://example.com/headSign".parse().expect("valid name"), AttributeNameKey::from(28))]),
            types: ResolvedNames::from_pairs([("StopTime", 18), ("Trip", 3)].map(|(name, key)| {
                (
                    format!("https://example.com/{name}").parse().expect("valid type name"),
                    EntityTypeNameKey::from(key),
                )
            })),
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

    fn page_sql(query: &CompiledEntityQuery) -> (String, Vec<Value>) {
        let (sql, values) = built(query, CountPolicy::Omit).select.build(PostgresQueryBuilder);
        (sql, values.0)
    }

    #[test]
    fn an_unanchored_id_pattern_beside_a_type_reads_the_identifier_trigrams() {
        let built = built(&by_pattern(".*_1196287$"), CountPolicy::Omit);
        assert_eq!(built.planning, StatementPlanning::GatedBranches);
        let (sql, values) = built.select.build(PostgresQueryBuilder);
        let candidates = sql
            .split(r#""candidates" AS  MATERIALIZED ("#)
            .nth(1)
            .and_then(|rest| rest.split(" LIMIT $").next())
            .unwrap_or_else(|| panic!("the index path: {sql}"));
        assert_eq!(candidates, r#"SELECT "entities"."entity_key" FROM "entities" WHERE "entities"."id" ~ $5"#);
        assert!(
            values.0.contains(&Value::BigUnsigned(Some(5_000))),
            "the walk tests 5,000 members: {:?}",
            values.0
        );
        assert!(values.0.contains(&Value::BigUnsigned(Some(1_001))), "{:?}", values.0);
        assert!(values.0.contains(&Value::BigUnsigned(Some(1_000))), "{:?}", values.0);
    }

    #[test]
    fn the_short_walk_tests_the_members_and_the_full_walk_keeps_the_pattern() {
        let (sql, _) = page_sql(&by_pattern(".*_1196287$"));
        let (short, _) = sql.split_once(r#", "candidates" AS "#).unwrap_or_else(|| panic!("a gated page: {sql}"));
        assert!(
            short.contains(r#"FROM "entity_type_members" WHERE "entity_type_name_key" = (18)) AS "selected" WHERE TRUE"#),
            "{short}"
        );
        assert!(
            short.contains(r#"AS "walked" WHERE "walked"."sort_0" ~ $"#),
            "the walk tests the identifier it projects: {short}"
        );
        let after_short = sql.split(r#"AS "answered""#).nth(1).expect("the index branch follows the short walk");
        let (indexed, walk) = after_short.split_once(r#"AS "indexed""#).expect("the index branch");
        assert!(
            indexed.contains(r#""selected"."entity_key" IN (SELECT "entity_key" FROM "candidates")"#),
            "{indexed}"
        );
        assert!(!indexed.contains(r#""selected"."id" ~"#), "the matches answer the pattern: {indexed}");
        assert!(walk.contains(r#""selected"."id" ~ $"#), "{walk}");
    }

    #[test]
    fn an_anchored_id_pattern_keeps_the_type_range() {
        for pattern in ["^urn:ngsi-ld:StopTime:99.*", "^urn:ngsi-ld:StopTime:1012_03_478_1196287$"] {
            let built = built(&by_pattern(pattern), CountPolicy::Omit);
            let sql = built.select.to_string(PostgresQueryBuilder);
            assert!(!sql.contains("short_walk") && !sql.contains("candidates"), "{pattern}: {sql}");
            assert_eq!(built.planning, StatementPlanning::Cached, "{pattern}");
        }
    }

    /// Driven by `entities`, the plain page can read the trigram index
    /// itself.
    #[test]
    fn an_id_pattern_without_a_type_keeps_the_plain_page() {
        let mut query = by_pattern(".*_1196287$");
        query.selection.selectors = vec![selector(None, ".*_1196287$")];
        let (sql, _) = page_sql(&query);
        assert!(!sql.contains("short_walk") && !sql.contains("entity_type_members"), "{sql}");
    }

    #[test]
    fn an_id_pattern_filtered_further_or_ordered_keeps_the_plain_page() {
        let mut filtered = by_pattern(".*_1196287$");
        filtered.selection.q_expr = Some(parse("\"https://example.com/headSign\">1").expect("valid q"));
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
        for query in [filtered, by_attrs, ordered, paired] {
            let built = built(&query, CountPolicy::Omit);
            let sql = built.select.to_string(PostgresQueryBuilder);
            assert!(!sql.contains("short_walk") && !sql.contains("candidates"), "{sql}");
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
        let page = built.select.to_string(PostgresQueryBuilder);
        assert!(page.contains(r#"COUNT(*) OVER () AS "total_count""#), "{page}");
        assert!(page.contains(r#""page"."gated_step" AS "gated_step""#), "{page}");
        let walked_at_once = uncounted
            .walked
            .expect("a page the short walk filled is counted by walking")
            .select
            .to_string(PostgresQueryBuilder);
        assert!(
            walked_at_once.contains(r#""selected"."id" ~ "#) && !walked_at_once.contains("candidates"),
            "{walked_at_once}"
        );
        let (sql, values) = uncounted.count.build(PostgresQueryBuilder);
        assert!(
            sql.starts_with(r#"WITH "candidates" AS  MATERIALIZED (SELECT "entities"."entity_key" FROM "entities" WHERE "entities"."id" ~ $1 LIMIT $2)"#),
            "{sql}"
        );
        let (_, arms) = sql.split_once(" THEN ").unwrap_or_else(|| panic!("a gated count: {sql}"));
        let (indexed, walked) = arms.split_once(" ELSE ").unwrap_or_else(|| panic!("a walk past the bound: {sql}"));
        assert!(
            indexed.contains(r#""selected"."entity_key" IN (SELECT "entity_key" FROM "candidates")"#),
            "{indexed}"
        );
        assert!(!indexed.contains(r#""selected"."id" ~"#), "{indexed}");
        assert!(walked.contains(r#""selected"."id" ~ $"#), "{walked}");
        assert!(values.0.contains(&Value::BigUnsigned(Some(10_001))), "{:?}", values.0);
        assert_eq!(built.total_planning, StatementPlanning::BoundValues);
    }
}
