//! The page of an entity query whose `q` holds a conjunct an index answers:
//! an equality on a sub-attribute, or a `~=` on an attribute's own value.
//!
//! The planner cannot price either term (it underestimates a broad one by
//! orders of magnitude), so the statement does not leave it the
//! choice between walking the selection and reading the index. It tries the
//! three in the order that stops soonest, and a gate on what the previous step
//! found switches each later one off:
//!
//! 1. a short walk: the first [`SHORT_WALK`] Entities of the selection in page
//!    order, each probed for the whole `q`. A broad predicate fills the page
//!    here in a few probes.
//! 2. the index: at most [`CANDIDATE_BOUND`] + 1 rows through
//!    `idx_ca_sub_eq_keys` or the trigram indexes
//!    ([`keyed_candidates`](andromeda_postgresql_query::q_predicate::keyed_term::keyed_candidates)),
//!    each rechecked, and the page cut from the selection narrowed to them,
//!    when they number at most [`CANDIDATE_BOUND`];
//! 3. the full walk: the selection in page order, each Entity probed, when
//!    more candidates qualify.
//!
//! Each step is one branch of a `UNION ALL` behind a
//! [`one_time_gate`](crate::one_time_gate). Every branch cuts the same window
//! over the same order, so whichever runs returns the page the plain
//! statement would.

use crate::{
    entity::{QFilter, build_entity_conditions, window_and_sort_keys},
    entity_page::{ProjectedPage, projected_page},
    entity_relations::EntityRelations,
    one_time_gate::{Candidates, bounded, column_of, count_of, materialized, within},
};
use andromeda_ngsi_ld_query::{
    count_policy::CountPolicy,
    entity::compiled::CompiledEntityQuery,
    filter::expression::Expr as QueryExpr,
    page_window::{PageStart, PageWindow},
    read_order::ReadOrder,
};
use andromeda_postgresql_query::{
    dictionary::query_names::QueryNames,
    entity_row_refs::EntityRowRefs,
    error::RenderError,
    q_predicate::{instance_scan::InstanceScan, keyed_term::KeyedCandidates, projection::compile},
};
use sea_query::{Alias, Asterisk, Condition, DynIden, Expr, ExprTrait, Iden, Query, SelectStatement, UnionType, WithClause};

/// How many Entities the short walk probes before the index is read.
pub const SHORT_WALK: u32 = 200;

/// The most candidates the index path hands to the page.
pub const CANDIDATE_BOUND: u64 = 1000;

/// The CTE holding the short walk's matches.
#[derive(Iden, Clone, Copy)]
#[iden = "short_walk"]
struct ShortWalk;

/// The name the short walk reads the Entities it probes under.
#[derive(Iden, Clone, Copy)]
#[iden = "walked"]
struct Walked;

/// The statement a page is cut from: its relations, in the branch they are
/// read in, and the query and window it answers.
pub(crate) struct PageSource<'a> {
    /// The relations the statement reads.
    pub(crate) relations: &'a EntityRelations,
    /// The branch schema, `None` for the live branch.
    pub(crate) schema: Option<&'a DynIden>,
    /// How the statement's fragments reach the rows.
    pub(crate) rows: &'a EntityRowRefs,
    /// The query.
    pub(crate) query: &'a CompiledEntityQuery,
    /// The window the page is.
    pub(crate) window: &'a PageWindow,
    /// The dictionary keys the query reads.
    pub(crate) names: &'a QueryNames,
}

impl PageSource<'_> {
    /// The page cut from the selection filtered by `condition` over `window`,
    /// projected, with the order it is read in.
    fn page(&self, condition: Condition, window: &PageWindow) -> Result<(ProjectedPage, ReadOrder), RenderError> {
        let mut page = Query::select();
        self.relations.clone().apply_from(&mut page, self.schema);
        page.cond_where(condition);
        let (keys, read_order) = window_and_sort_keys(&mut page, self.rows, self.query, window, &self.names.attributes)?;
        Ok((projected_page(page, self.rows, keys, CountPolicy::Omit), read_order))
    }

    /// The selection filtered as `q` says, the `q` term `keyed` answers left
    /// out or kept.
    fn condition(&self, q: QFilter<'_>) -> Result<Condition, RenderError> {
        build_entity_conditions(self.query, self.names, self.rows, &self.relations.source, q)
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
    let need = match source.window.start {
        PageStart::Offset(offset) => u64::from(offset) + u64::from(source.window.limit),
        PageStart::Cursor(_) => u64::from(source.window.limit),
    };
    let short = short_walk(source, q_expr, need)?;
    let walked_enough = count_of(ShortWalk).gte(need);
    let walked_short = count_of(ShortWalk).lt(need);
    let few_candidates = within(Candidates, CANDIDATE_BOUND);
    let many_candidates = count_of(Candidates).gt(CANDIDATE_BOUND);

    let (indexed, read_order) = source.page(
        source
            .condition(QFilter::Except(keyed.term))?
            .add(source.rows.entity_key_column().in_subquery(column_of(Candidates)))
            .add(walked_short.clone()) // clone: both later branches are gated on the short walk
            .add(few_candidates),
        source.window,
    )?;
    let (walk, _) = source.page(source.condition(QFilter::Whole)?.add(walked_short).add(many_candidates), source.window)?;
    let order = indexed.order;

    let mut answered = Query::select();
    answered.column(Asterisk).from(ShortWalk).and_where(walked_enough);
    for (column, key_order) in &order {
        key_order.order_by(&mut answered, Expr::col(column.clone())); // clone: each branch orders by the same columns
    }
    match source.window.start {
        PageStart::Offset(offset) => {
            answered.offset(u64::from(offset));
        }
        PageStart::Cursor(_) => {}
    }
    answered.limit(u64::from(source.window.limit));

    let mut union = wrapped(answered.take(), "answered");
    union
        .union(UnionType::All, wrapped(indexed.select, "indexed"))
        .union(UnionType::All, wrapped(walk.select, "walk"))
        .with_cte(
            WithClause::new()
                .cte(materialized(short, ShortWalk))
                .cte(materialized(bounded(keyed.select, CANDIDATE_BOUND), Candidates))
                .to_owned(),
        );
    Ok((ProjectedPage { select: union.take(), order }, read_order))
}

/// The short walk: the first [`SHORT_WALK`] Entities of the selection in page
/// order, from where the window starts, that the whole `q` keeps, at most
/// `need` of them.
///
/// The selection is walked without `q`, and `q` probes the Entities it yields,
/// so the walk visits [`SHORT_WALK`] Entities at most whatever `q` matches.
fn short_walk(source: &PageSource<'_>, q_expr: &QueryExpr, need: u64) -> Result<SelectStatement, RenderError> {
    let start = match &source.window.start {
        PageStart::Offset(_) => PageStart::Offset(0),
        // clone: the walk starts where the window does
        PageStart::Cursor(cursor) => PageStart::Cursor(cursor.clone()),
    };
    let window = PageWindow { limit: SHORT_WALK, start };
    let (walked, _) = source.page(source.condition(QFilter::Omitted)?, &window)?;
    let ProjectedPage { select: walked, order } = walked;
    let walked_rows = EntityRowRefs::unaliased(source.schema.cloned()).keyed_by(Walked);
    let mut short = Query::select();
    short
        .column((Walked, Asterisk))
        .from_subquery(walked, Walked)
        .cond_where(compile(q_expr, &InstanceScan::Current(&walked_rows), source.names));
    for (column, key_order) in order {
        key_order.order_by(&mut short, Expr::col((Walked, column)));
    }
    short.limit(need);
    Ok(short.take())
}

/// `SELECT * FROM (<branch>) AS <alias>`, so the branch keeps its own order
/// and window inside the union.
fn wrapped(branch: SelectStatement, alias: &str) -> SelectStatement {
    Query::select().column(Asterisk).from_subquery(branch, Alias::new(alias)).take()
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
                [("temperature", 12), ("source", 13)]
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
        assert_eq!(built.planning, StatementPlanning::BoundValues);
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
    /// keyed page is planned with its bound values.
    #[test]
    fn a_keyed_page_is_planned_with_its_bound_values() {
        let built = build_query(
            &ContextTarget::Live,
            &query("\"https://example.com/temperature\".\"https://example.com/source\"==\"uncalibrated\""),
            &PageWindow::offset(100, 0),
            CountPolicy::Omit,
            &names(),
        )
        .expect("valid entity query");
        assert_eq!(built.planning, StatementPlanning::BoundValues);
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
