//! The page of an entity query whose `q` holds a conjunct an index answers:
//! an equality on a sub-attribute, a `~=` on an attribute's own value, an
//! ordering or a range on an attribute's own `DateTime` or `Date` value, the
//! bare existence of an attribute, or a Linked Entity hop.
//!
//! The planner cannot price either term (it underestimates a broad one by
//! orders of magnitude), so the page is a [`gated_page`]:
//!
//! 1. the short walk probes the first [`SHORT_WALK`] Entities of the
//!    selection in page order for the whole `q`;
//! 2. the index reads at most [`CANDIDATE_BOUND`] + 1 rows through
//!    `idx_ca_sub_eq_keys` or the trigram indexes, each rechecked, at most
//!    [`RANGED_CANDIDATE_BOUND`] + 1 from a range of a typed value index, or
//!    at most [`PROBED_CANDIDATE_BOUND`] + 1 carriers of an attribute name,
//!    each probed for a standing instance, or at most
//!    [`LINKED_CANDIDATE_BOUND`] + 1 Entities naming a linked entity the
//!    hop's inner term keeps
//!    ([`keyed_candidates`](andromeda_postgresql_query::q_predicate::keyed_term::keyed_candidates)),
//!    and the page is cut from the selection narrowed to them under the rest
//!    of `q`;
//! 3. the full walk probes each Entity of the selection for the whole `q`.

use crate::{
    entity::QFilter,
    entity_page::ProjectedPage,
    gated_page::{IndexOrWalk, PageSource, ShortWalk, gated_page, walked_rows},
};
use andromeda_ngsi_ld_query::{filter::expression::Expr as QueryExpr, read_order::ReadOrder};
use andromeda_postgresql_query::{
    error::RenderError,
    q_predicate::{
        instance_scan::InstanceScan,
        keyed_term::{CandidateRead, KeyedCandidates},
        projection::compile,
    },
};

/// How many Entities the short walk probes before the index is read.
pub const SHORT_WALK: u32 = 200;

/// The most rechecked candidates the index path hands to the page.
pub const CANDIDATE_BOUND: u64 = 1000;

/// The most candidates a range of a typed value index hands to the page.
///
/// A ranged candidate costs an index entry and a heap visit, with no recheck,
/// so the index answers a term whose matches cluster late in page order far
/// sooner than the walk well past the rechecked bound.
pub const RANGED_CANDIDATE_BOUND: u64 = 50_000;

/// The most carriers of an attribute name the index path hands to the page.
///
/// A carrier costs a posting of the GIN over the search row's names and one
/// probe of the grain constraint, far less than a value recheck, so the bound
/// sits well past the rechecked one: an Entity that holds the attribute among
/// a selection mostly without it is reached through the index, not the walk.
pub const PROBED_CANDIDATE_BOUND: u64 = 10_000;

/// The most Entities naming a linked entity the index path hands to the page.
///
/// A linked candidate costs an entry of the object's index and a heap visit,
/// with no recheck. Past the bound the page walks the selection serially and
/// probes each Entity's Relationship outward, which costs several times the
/// candidates of a hop matching tens of thousands of Entities, so the bound
/// sits at the ranged one.
pub const LINKED_CANDIDATE_BOUND: u64 = 50_000;

/// The most candidates the index path hands to the page, by what one costs to
/// read.
const fn page_bound(read: CandidateRead) -> u64 {
    match read {
        CandidateRead::Rechecked => CANDIDATE_BOUND,
        CandidateRead::Ranged => RANGED_CANDIDATE_BOUND,
        CandidateRead::Probed => PROBED_CANDIDATE_BOUND,
        CandidateRead::Linked => LINKED_CANDIDATE_BOUND,
    }
}

/// The projected page of `source` for a `q` with a keyed term, and the order
/// its rows come back in.
///
/// # Errors
///
/// Fails as rendering the plain page fails: a reference geometry with no EWKB
/// spelling, or a query-language rule a renderer decides first.
pub(crate) fn keyed_page(source: &PageSource<'_>, q_expr: &QueryExpr, keyed: KeyedCandidates<'_>) -> Result<(ProjectedPage, ReadOrder), RenderError> {
    let walked = walked_rows(source.schema);
    let short = ShortWalk {
        source: *source,
        walked: source.condition(QFilter::Omitted)?,
        probe: compile(q_expr, &InstanceScan::Current(&walked), source.names),
        length: SHORT_WALK,
    };
    let rest = IndexOrWalk {
        candidates: keyed.select,
        bound: page_bound(keyed.read),
        narrowed: source.condition(QFilter::Except(keyed.term))?,
        whole: source.condition(QFilter::Whole)?,
    };
    gated_page(source, short, rest)
}

#[cfg(test)]
mod tests {
    use crate::{entity::build_query, entity_total::EntityTotal};
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_query::{
        count_policy::CountPolicy,
        entity::compiled::CompiledEntityQuery,
        entity_selection::EntitySelection,
        filter::parser::parse,
        page_window::PageWindow,
    };
    use andromeda_postgresql_query::{
        attribute_name_key::AttributeNameKey,
        dictionary::{query_names::QueryNames, resolved_names::ResolvedNames},
    };
    use andromeda_postgresql_seaquery::statement_planning::StatementPlanning;
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, Value};

    fn names() -> QueryNames {
        QueryNames {
            attributes: ResolvedNames::from_pairs(
                [("temperature", 12), ("source", 13), ("hasTrip", 14)]
                    .map(|(name, key)| (format!("https://example.com/{name}").parse().expect("valid name"), AttributeNameKey::from(key))),
            ),
            types: ResolvedNames::default(),
        }
    }

    fn query(q: &str) -> CompiledEntityQuery {
        let mut query = CompiledEntityQuery::builder().selection(EntitySelection::builder().build()).build();
        query.selection.q_expr = Some(parse(q).expect("valid q"));
        query
    }

    /// The page tries the short walk, then the bounded index, then the full
    /// walk, each gated on what the one before it found.
    #[test]
    fn a_keyed_equality_walks_short_then_probes_the_index() {
        let built = build_query(
            &ContextTarget::Live,
            &query("\"https://example.com/temperature\".\"https://example.com/source\"==\"sensor-43\""),
            &PageWindow::offset(100, 0),
            CountPolicy::Emit,
            &names(),
        )
        .expect("valid entity query");
        let (sql, _) = built.select.build(PostgresQueryBuilder);
        let short = sql
            .find(r#"WITH "short_walk" AS  MATERIALIZED (SELECT "walked".*"#)
            .unwrap_or_else(|| panic!("the short walk: {sql}"));
        let candidates = sql
            .find(r#""candidates" AS  MATERIALIZED (SELECT "ca"."entity_key""#)
            .unwrap_or_else(|| panic!("the index path: {sql}"));
        assert!(short < candidates, "{sql}");
        assert!(sql.contains(r#"sub_eq_keys("ca"."attribute_name_key", "ca"."sub_attributes") && "#), "{sql}");
        assert!(sql.contains(r#"(SELECT COUNT(*) FROM "short_walk") >= $"#), "{sql}");
        assert!(sql.contains(r#"(SELECT COUNT(*) FROM "candidates") <= $"#), "{sql}");
        assert!(sql.contains(r#"(SELECT COUNT(*) FROM "candidates") > $"#), "{sql}");
        assert!(sql.contains(r#""entities"."entity_key" IN (SELECT "entity_key" FROM "candidates")"#), "{sql}");
        assert_eq!(sql.matches("UNION ALL").count(), 2, "{sql}");
        assert!(sql.contains("OFFSET"), "the full walk probes each Entity: {sql}");
        assert!(
            matches!(built.total, EntityTotal::Counted(_)),
            "the total is counted on its own, since the short walk sees part of the selection"
        );
    }

    #[test]
    fn the_page_reads_one_candidate_past_its_bound() {
        let built = build_query(
            &ContextTarget::Live,
            &query("\"https://example.com/temperature\"~=\"text-71.*\""),
            &PageWindow::offset(100, 0),
            CountPolicy::Omit,
            &names(),
        )
        .expect("valid entity query");
        let (sql, values) = built.select.build(PostgresQueryBuilder);
        let candidates = sql
            .split_once(r#""candidates" AS  MATERIALIZED ("#)
            .map_or_else(|| panic!("the index path: {sql}"), |(_, rest)| rest);
        assert!(candidates.contains(r#") AS "matches" LIMIT $"#), "{candidates}");
        assert!(values.0.contains(&Value::BigUnsigned(Some(1001))), "{:?}", values.0);
    }

    /// A range of a typed value index is a match as it is read, so the page
    /// reads more of them before it walks.
    #[test]
    fn a_date_time_ordering_reads_its_candidates_under_the_ranged_bound() {
        let built = build_query(
            &ContextTarget::Live,
            &query("\"https://example.com/temperature\">2024-01-15T12:00:00Z"),
            &PageWindow::offset(100, 0),
            CountPolicy::Omit,
            &names(),
        )
        .expect("valid entity query");
        assert_eq!(built.planning, StatementPlanning::GatedBranches);
        let (sql, values) = built.select.build(PostgresQueryBuilder);
        let candidates = sql
            .split(r#""candidates" AS  MATERIALIZED ("#)
            .nth(1)
            .and_then(|rest| rest.split(r#") AS "matches""#).next())
            .unwrap_or_else(|| panic!("the index path: {sql}"));
        assert!(candidates.contains(r#""ca"."value_date_time" > $"#), "{candidates}");
        assert_eq!(candidates.matches("UNION ALL").count(), 1, "the typed and the language branch: {candidates}");
        assert!(values.0.contains(&Value::BigUnsigned(Some(50_001))), "{:?}", values.0);
        assert!(values.0.contains(&Value::BigUnsigned(Some(50_000))), "{:?}", values.0);
    }

    /// Clause 4.9 `q=attr`: the carriers of the name are each probed for a
    /// standing instance, so the index branch leaves the term out and the full
    /// walk keeps it.
    #[test]
    fn a_bare_existence_pages_through_the_carriers_of_its_name() {
        let built = build_query(
            &ContextTarget::Live,
            &query("\"https://example.com/temperature\""),
            &PageWindow::offset(100, 0),
            CountPolicy::Omit,
            &names(),
        )
        .expect("valid entity query");
        assert_eq!(built.planning, StatementPlanning::GatedBranches);
        let (sql, values) = built.select.build(PostgresQueryBuilder);
        let candidates = sql
            .split(r#""candidates" AS  MATERIALIZED ("#)
            .nth(1)
            .and_then(|rest| rest.split(r#") AS "matches""#).next())
            .unwrap_or_else(|| panic!("the index path: {sql}"));
        assert!(candidates.contains(r#""entity_search"."attribute_name_keys" @> (ARRAY[12])"#), "{candidates}");
        assert!(candidates.contains(r#""ca"."entity_key" = "entity_search"."entity_key""#), "{candidates}");
        assert!(values.0.contains(&Value::BigUnsigned(Some(10_001))), "{:?}", values.0);
        assert!(values.0.contains(&Value::BigUnsigned(Some(10_000))), "{:?}", values.0);
        let after_short = sql.split(r#"AS "answered""#).nth(1).expect("the index branch follows the short walk");
        let (indexed, walk) = after_short.split_once(r#"AS "indexed""#).expect("the index branch");
        assert!(!indexed.contains("current_attributes"), "{indexed}");
        assert!(walk.contains(r#"FROM "current_attributes" AS "ca""#), "{walk}");
    }

    /// A Date is held as its fixed-width text, so its candidates range over the
    /// string index, and the index branch leaves the ordering out.
    /// The hop is read from the linked entities inward, and the index branch
    /// leaves it out of the filter it applies to the candidates, while the
    /// walk past the bound probes it outward.
    #[test]
    fn a_linked_entity_hop_pages_through_the_instances_naming_its_linked_matches() {
        let built = build_query(
            &ContextTarget::Live,
            &query("\"https://example.com/hasTrip\"{\"https://example.com/temperature\"}==\"x\""),
            &PageWindow::offset(100, 0),
            CountPolicy::Omit,
            &names(),
        )
        .expect("valid entity query");
        assert_eq!(built.planning, StatementPlanning::GatedBranches);
        let (sql, values) = built.select.build(PostgresQueryBuilder);
        let candidates = sql
            .split(r#""candidates" AS  MATERIALIZED ("#)
            .nth(1)
            .and_then(|rest| rest.split(r#") AS "matches""#).next())
            .unwrap_or_else(|| panic!("the index path: {sql}"));
        assert!(
            candidates.contains(r#"FROM "entities" AS "le1" JOIN "current_attributes" AS "ca_ln1""#),
            "{candidates}"
        );
        assert!(candidates.contains(r#""ca_ln1"."value_uri" = "le1"."id""#), "{candidates}");
        assert!(values.0.contains(&Value::BigUnsigned(Some(50_001))), "{:?}", values.0);
        assert!(values.0.contains(&Value::BigUnsigned(Some(50_000))), "{:?}", values.0);
        let after_short = sql.split(r#"AS "answered""#).nth(1).expect("the index branch follows the short walk");
        let (indexed, walk) = after_short.split_once(r#"AS "indexed""#).expect("the index branch");
        assert!(!indexed.contains("ca_ln1"), "the candidates answer the hop: {indexed}");
        assert!(walk.contains("JOIN jsonb_array_elements_text("), "the walk probes the hop outward: {walk}");
    }

    #[test]
    fn a_date_ordering_pages_through_the_string_index() {
        let built = build_query(
            &ContextTarget::Live,
            &query("\"https://example.com/temperature\"<=2024-01-15"),
            &PageWindow::offset(100, 0),
            CountPolicy::Omit,
            &names(),
        )
        .expect("valid entity query");
        let (sql, _) = built.select.build(PostgresQueryBuilder);
        let after_short = sql.split(r#"AS "answered""#).nth(1).expect("the index branch follows the short walk");
        let (indexed, walk) = after_short.split_once(r#"AS "indexed""#).expect("the index branch");
        assert!(
            indexed.contains(r#""entities"."entity_key" IN (SELECT "entity_key" FROM "candidates")"#),
            "{indexed}"
        );
        assert!(!indexed.contains(r#""ca"."value_string" <= $"#), "{indexed}");
        assert!(walk.contains(r#""ca"."value_string" <= $"#), "{walk}");
    }

    /// The index branch answers the keyed term with its candidates, so its
    /// filter leaves that term out and keeps the others.
    #[test]
    fn the_index_branch_leaves_out_the_term_its_candidates_answer() {
        let built = build_query(
            &ContextTarget::Live,
            &query("\"https://example.com/temperature\".\"https://example.com/source\"==\"sensor-43\";\"https://example.com/temperature\">1"),
            &PageWindow::offset(100, 0),
            CountPolicy::Omit,
            &names(),
        )
        .expect("valid entity query");
        let (sql, _) = built.select.build(PostgresQueryBuilder);
        let indexed = sql.split(r#"AS "answered""#).nth(1).expect("the index branch follows the short walk");
        let indexed = indexed.split(r#"AS "indexed""#).next().expect("the index branch");
        assert_eq!(indexed.matches("UNNEST(").count(), 0, "{indexed}");
        assert!(indexed.contains(r#""ca"."value_number" > "#), "{indexed}");
    }

    /// A `~=` on an own value tries the short walk, then the trigram
    /// candidates, then the full walk, like a keyed equality.
    #[test]
    fn a_pattern_on_an_own_value_walks_short_then_probes_the_trigram_indexes() {
        let built = build_query(
            &ContextTarget::Live,
            &query("\"https://example.com/temperature\"~=\"text-71.*\";\"https://example.com/temperature\"!=\"x\""),
            &PageWindow::offset(100, 0),
            CountPolicy::Omit,
            &names(),
        )
        .expect("valid entity query");
        assert_eq!(built.planning, StatementPlanning::GatedBranches);
        let (sql, _) = built.select.build(PostgresQueryBuilder);
        let candidates = sql
            .split(r#""candidates" AS  MATERIALIZED ("#)
            .nth(1)
            .and_then(|rest| rest.split(r#") AS "matches""#).next())
            .unwrap_or_else(|| panic!("the index path: {sql}"));
        assert!(candidates.contains(r#""ca"."value_string" ~ $"#), "{candidates}");
        assert!(candidates.contains(r#""ca"."value_long_text" ~ $"#), "{candidates}");
        assert_eq!(candidates.matches("UNION ALL").count(), 2, "{candidates}");
        assert!(sql.contains(r#""entities"."entity_key" IN (SELECT "entity_key" FROM "candidates")"#), "{sql}");
        assert!(sql.contains(r#"(SELECT COUNT(*) FROM "candidates") > $"#), "{sql}");
    }

    /// Every branch cuts the same window over the same order, so whichever
    /// gate opens returns the page the plain statement would.
    #[test]
    fn a_pattern_page_cuts_the_same_window_as_the_plain_statement() {
        let q = "\"https://example.com/temperature\"~=\"text-71.*\"";
        let window = PageWindow::offset(20, 40);
        let keyed = build_query(&ContextTarget::Live, &query(q), &window, CountPolicy::Omit, &names()).expect("valid entity query");
        let (sql, _) = keyed.select.build(PostgresQueryBuilder);
        let branches: Vec<&str> = sql.split("UNION ALL (SELECT * FROM (").skip(1).collect();
        assert_eq!(branches.len(), 2, "the index and the full walk follow the short walk: {sql}");
        for branch in branches {
            assert!(branch.contains(r#"ORDER BY "sort_0" ASC"#), "{branch}");
            assert!(branch.contains("LIMIT $") && branch.contains("OFFSET $"), "{branch}");
        }
    }

    /// The index branch answers the pattern with its candidates, so its filter
    /// leaves the pattern out, and the full walk keeps it.
    #[test]
    fn the_pattern_candidates_are_rechecked_by_the_term() {
        let built = build_query(
            &ContextTarget::Live,
            &query("\"https://example.com/temperature\"~=\"text-71.*\""),
            &PageWindow::offset(100, 0),
            CountPolicy::Omit,
            &names(),
        )
        .expect("valid entity query");
        let (sql, _) = built.select.build(PostgresQueryBuilder);
        let after_short = sql.split(r#"AS "answered""#).nth(1).expect("the index branch follows the short walk");
        let (indexed, walk) = after_short.split_once(r#"AS "indexed""#).expect("the index branch");
        assert!(!indexed.contains("value_string"), "{indexed}");
        assert!(walk.contains(r#""ca"."value_string" ~ $"#), "{walk}");
    }

    /// A pattern on a sub-attribute has no key, so the page is the plain walk.
    #[test]
    fn a_sub_attribute_pattern_pages_without_the_index() {
        let built = build_query(
            &ContextTarget::Live,
            &query("\"https://example.com/temperature\".\"https://example.com/source\"~=\"^sensor-4[0-9]$\""),
            &PageWindow::offset(100, 0),
            CountPolicy::Omit,
            &names(),
        )
        .expect("valid entity query");
        let (sql, _) = built.select.build(PostgresQueryBuilder);
        assert!(!sql.contains("short_walk") && !sql.contains("sub_eq_keys"), "{sql}");
        assert!(sql.contains("OFFSET"), "{sql}");
    }

    /// Without the operand a generic plan cannot estimate the key overlap and
    /// reads the attribute's rows through its name index instead, so the
    /// keyed page is planned with its bound values, and its gated-off
    /// branches launch no parallel worker.
    #[test]
    fn a_keyed_page_is_planned_with_its_bound_values_and_runs_serially() {
        let built = build_query(
            &ContextTarget::Live,
            &query("\"https://example.com/temperature\".\"https://example.com/source\"==\"uncalibrated\""),
            &PageWindow::offset(100, 0),
            CountPolicy::Omit,
            &names(),
        )
        .expect("valid entity query");
        assert_eq!(built.planning, StatementPlanning::GatedBranches);
    }

    #[test]
    fn a_page_without_a_keyed_term_takes_any_cached_plan() {
        for q in [
            "\"https://example.com/temperature\".\"https://example.com/source\"~=\"^sensor-4[0-9]$\"",
            "\"https://example.com/temperature\">1",
        ] {
            let built = build_query(&ContextTarget::Live, &query(q), &PageWindow::offset(100, 0), CountPolicy::Omit, &names()).expect("valid entity query");
            assert_eq!(built.planning, StatementPlanning::Cached, "{q}");
        }
    }
}
