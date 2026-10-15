//! The page of a selection by `attrs` alone, which keeps only the Entities
//! that carry at least one of the Attributes (clause 5.7.2.4).
//!
//! The carriers of an Attribute are one key of the GIN over the search row's
//! names, which yields them in no order, while the page wants them in
//! identifier order (clause 5.5.9). The planner assumes the carriers spread
//! evenly through `uq_entities_id`, so it walks that index and filters each
//! Entity; when they sort late, as the Entities of one type do behind those
//! of a larger one, it visits every Entity before them. The page is therefore
//! a [`gated_page`]:
//!
//! 1. the short walk visits the first [`CARRIER_SHORT_WALK`] Entities in
//!    identifier order, each probed through its search row;
//! 2. the index reads at most [`CARRIER_BOUND`] + 1 carriers, and the page is
//!    cut from them, sorted;
//! 3. the full walk filters every Entity in identifier order.
//!
//! Only a page in identifier order is cut this way. Under an `orderBy` the
//! plain page sorts the selection whichever relation it reads first, so the
//! walk is never the planner's choice for it.

use crate::{
    carried_names::attrs_condition,
    entity::{QFilter, build_entity_conditions},
    entity_page::ProjectedPage,
    entity_relations::{Demand, EntityRelations, RowSource},
    gated_page::{IndexOrWalk, PageSource, ShortWalk, Walked, gated_page},
};
use andromeda_ngsi_ld_query::{entity::compiled::CompiledEntityQuery, entity_selection::ConsideredEntities, read_order::ReadOrder};
use andromeda_postgresql_query::{dictionary::query_names::QueryNames, entity_row_refs::EntityRowRefs, error::RenderError};
use andromeda_postgresql_schema::current::EntitySearch;
use sea_query::{Condition, DynIden, Expr, ExprTrait, Iden, Query, SelectStatement};

/// How many Entities the short walk visits before the carriers are read.
///
/// A visit costs one primary-key probe of the search row, so the walk is
/// longer than a keyed page's, whose visits each probe the Entity's attribute
/// rows.
pub const CARRIER_SHORT_WALK: u32 = 5_000;

/// The most carriers the index step reads and sorts.
pub const CARRIER_BOUND: u64 = 50_000;

/// The name the short walk's probe reads a walked Entity's search row under.
#[derive(Iden, Clone, Copy)]
#[iden = "probed"]
struct Probed;

/// `SELECT "entity_key" FROM "entity_search" WHERE <attrs>`, every carrier of
/// the request's Attributes, when the query selects by `attrs` alone in
/// identifier order and the dictionary has recorded one of the Attributes.
///
/// An Attribute no Entity has carried leaves the other terms to decide, and
/// when every one is unrecorded the plain page is a constant `FALSE`.
pub(crate) fn carriers(query: &CompiledEntityQuery, names: &QueryNames, schema: Option<&DynIden>) -> Option<SelectStatement> {
    let selection = &query.selection;
    let alone = match selection.considered {
        ConsideredEntities::Selected => {
            !selection.selectors_constrain()
                && selection.q_expr.is_none()
                && selection.geo.is_none()
                && selection.scope_q.is_none()
                && query.ordering.terms.is_empty()
        }
        ConsideredEntities::NoEntity => false,
    };
    let recorded = selection.attrs.iter().any(|attr| names.attributes.get_str(attr.as_str()).is_some());
    if !(alone && recorded) {
        return None;
    }
    let rows = EntityRowRefs::unaliased(schema.cloned());
    let carried = attrs_condition(selection, names, &rows)?;
    let mut carriers = Query::select();
    carriers
        .expr(rows.search_column(EntitySearch::EntityKey))
        .from(rows.table(EntitySearch::Table))
        .cond_where(carried);
    Some(carriers.take())
}

/// The projected page of `source`, a selection by `attrs` alone, cut from its
/// `carriers`, and the order its rows come back in.
///
/// # Errors
///
/// Fails as rendering the plain page fails.
pub(crate) fn carrier_page(source: &PageSource<'_>, carriers: SelectStatement) -> Result<(ProjectedPage, ReadOrder), RenderError> {
    let identity = EntityRelations {
        source: RowSource::Entities,
        search: Demand::NotJoined,
    };
    let probed = EntityRowRefs::aliased(source.schema.cloned(), Walked, Probed);
    let mut probe = Query::select();
    probe.expr(Expr::val(1)).from_as(probed.table(EntitySearch::Table), Probed).cond_where(
        build_entity_conditions(source.query, source.names, &probed, &RowSource::Entities, QFilter::Whole)?
            .add(Expr::col((Probed, EntitySearch::EntityKey)).eq(probed.entity_key_column())),
    );
    let short = ShortWalk {
        source: PageSource {
            relations: &identity,
            ..*source
        },
        walked: Condition::all(),
        probe: Condition::all().add(Expr::exists(probe.take())),
        length: CARRIER_SHORT_WALK,
    };
    let whole = source.condition(QFilter::Whole)?;
    let rest = IndexOrWalk {
        candidates: carriers,
        bound: CARRIER_BOUND,
        // clone: the carriers are rechecked by the filter the full walk applies
        narrowed: whole.clone(),
        whole,
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
        entity_selection::{EntitySelection, EntitySelector},
        filter::parser::parse,
        ordering::term::{CompiledOrderTerm, OrderDirection, OrderTarget},
        page_window::PageWindow,
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

    fn iri(value: &str) -> IriBuf {
        IriBuf::new(value.to_string()).expect("valid IRI")
    }

    /// `headSign` at key 28 and `temperature` at key 12; the type `Trip` at
    /// key 3.
    fn names() -> QueryNames {
        QueryNames {
            attributes: ResolvedNames::from_pairs(
                [("headSign", 28), ("temperature", 12)]
                    .map(|(name, key)| (format!("https://example.com/{name}").parse().expect("valid name"), AttributeNameKey::from(key))),
            ),
            types: ResolvedNames::from_pairs([("https://example.com/Trip".parse().expect("valid type name"), EntityTypeNameKey::from(3))]),
        }
    }

    fn by_attrs(attrs: &[&str]) -> CompiledEntityQuery {
        let mut query = CompiledEntityQuery::builder().selection(EntitySelection::builder().build()).build();
        query.selection.selectors = vec![EntitySelector::builder().build()];
        query.selection.attrs = attrs.iter().map(|attr| iri(&format!("https://example.com/{attr}"))).collect();
        query
    }

    fn page_sql(query: &CompiledEntityQuery, window: &PageWindow) -> (String, Vec<Value>) {
        let built = build_query(&ContextTarget::Live, query, window, CountPolicy::Omit, &names()).expect("valid entity query");
        let (sql, values) = built.select.build(PostgresQueryBuilder);
        (sql, values.0)
    }

    #[test]
    fn an_attrs_only_selection_walks_short_then_reads_the_attribute_index() {
        let (sql, values) = page_sql(&by_attrs(&["headSign"]), &PageWindow::offset(100, 0));
        let (ctes, branches) = sql
            .split_once(" SELECT * FROM (SELECT * FROM \"short_walk\"")
            .unwrap_or_else(|| panic!("a gated page: {sql}"));
        let (short, candidates) = ctes
            .split_once(r#", "candidates" AS  MATERIALIZED "#)
            .unwrap_or_else(|| panic!("the carriers: {sql}"));
        assert!(
            short.contains(r#"FROM (SELECT "entities"."entity_key", "entities"."id" AS "sort_0" FROM "entities" WHERE TRUE ORDER BY "sort_0" ASC LIMIT $"#),
            "the walk visits Entities without filtering them: {short}"
        );
        assert!(
            short.contains("WHERE EXISTS(SELECT $")
                && short.contains(r#"FROM "entity_search" AS "probed" WHERE ("probed"."attribute_name_keys" @> (ARRAY[28])) AND "probed"."entity_key" = "walked"."entity_key")"#),
            "each walked Entity is probed through its search row: {short}"
        );
        assert_eq!(
            candidates,
            r#"(SELECT "entity_search"."entity_key" FROM "entity_search" WHERE "entity_search"."attribute_name_keys" @> (ARRAY[28]) LIMIT $5)"#
        );
        assert!(
            branches.contains(r#""entities"."entity_key" IN (SELECT "entity_key" FROM "candidates")"#),
            "the index step cuts the page from the carriers: {branches}"
        );
        assert_eq!(sql.matches("UNION ALL").count(), 2, "{sql}");
        assert!(values.contains(&Value::BigUnsigned(Some(5_000))), "the walk visits 5,000 Entities: {values:?}");
        assert!(
            values.contains(&Value::BigUnsigned(Some(50_001))),
            "the carriers are read one past their bound: {values:?}"
        );
        assert!(values.contains(&Value::BigUnsigned(Some(50_000))), "{values:?}");
    }

    #[test]
    fn a_carrier_page_is_planned_with_its_bound_values_and_runs_serially() {
        let built = build_query(
            &ContextTarget::Live,
            &by_attrs(&["headSign"]),
            &PageWindow::offset(100, 0),
            CountPolicy::Omit,
            &names(),
        )
        .expect("valid entity query");
        assert_eq!(built.planning, StatementPlanning::GatedBranches);
    }

    /// Clause 6.3.13: the short walk sees part of the selection, so the total
    /// is its own count over every carrier.
    #[test]
    fn a_counted_carrier_page_counts_the_whole_selection_on_its_own() {
        let built = build_query(
            &ContextTarget::Live,
            &by_attrs(&["headSign"]),
            &PageWindow::offset(100, 0),
            CountPolicy::Emit,
            &names(),
        )
        .expect("valid entity query");
        let EntityTotal::Counted(count) = built.total else {
            panic!("a carrier page carries no total on its rows");
        };
        let count = count.to_string(PostgresQueryBuilder);
        assert!(count.starts_with("SELECT COUNT(*) FROM "), "{count}");
        assert!(count.contains(r#""entity_search"."attribute_name_keys" @> (ARRAY[28])"#), "{count}");
        assert!(!count.contains("candidates") && !count.contains("short_walk"), "{count}");
        assert_eq!(built.total_planning, StatementPlanning::Cached);
        assert!(!built.select.to_string(PostgresQueryBuilder).contains("total_count"));
    }

    /// Clause 5.7.2.4: an Entity carrying any one of the Attributes matches,
    /// and a name no Entity has carried selects nothing.
    #[test]
    fn the_carriers_of_several_attributes_are_read_as_one_disjunction() {
        let (sql, _) = page_sql(&by_attrs(&["headSign", "temperature", "neverWritten"]), &PageWindow::offset(100, 0));
        assert!(
            sql.contains(concat!(
                r#""candidates" AS  MATERIALIZED (SELECT "entity_search"."entity_key" FROM "entity_search" WHERE "#,
                r#"("entity_search"."attribute_name_keys" @> (ARRAY[28])) OR ("entity_search"."attribute_name_keys" @> (ARRAY[12])) OR (FALSE) LIMIT $"#
            )),
            "{sql}"
        );
    }

    #[test]
    fn a_cursor_page_of_carriers_walks_from_its_cursor() {
        let boundary = "urn:ngsi-ld:Trip:042".parse().expect("valid identifier");
        let (sql, _) = page_sql(&by_attrs(&["headSign"]), &PageWindow::after(20, boundary));
        let short = sql
            .split_once(r#", "candidates" AS "#)
            .map_or_else(|| panic!("a gated page: {sql}"), |(short, _)| short);
        assert!(short.contains(r#"FROM "entities" WHERE "entities"."id" > $"#), "{short}");
        assert!(!sql.contains("OFFSET"), "a cursor page skips nothing: {sql}");
    }

    #[test]
    fn a_page_deeper_than_the_short_walk_visits_no_entity_before_the_carriers() {
        let (sql, values) = page_sql(&by_attrs(&["headSign"]), &PageWindow::offset(100, 30_000));
        assert!(sql.contains(r#"FROM "entities" WHERE TRUE ORDER BY "sort_0" ASC LIMIT $1 OFFSET $2"#), "{sql}");
        assert_eq!(values.first(), Some(&Value::BigUnsigned(Some(0))), "{values:?}");
        assert!(!values.contains(&Value::BigUnsigned(Some(5_000))), "{values:?}");
    }

    #[test]
    fn an_attrs_selection_beside_a_type_keeps_the_type_walk() {
        let mut query = by_attrs(&["headSign"]);
        query.selection.selectors = vec![
            EntitySelector::builder()
                .type_expr(Some(TypeExpr::Type(iri("https://example.com/Trip"))))
                .build(),
        ];
        let (sql, _) = page_sql(&query, &PageWindow::offset(100, 0));
        assert!(!sql.contains("short_walk") && !sql.contains("candidates"), "{sql}");
        assert!(sql.contains(r#"FROM "entity_type_members""#), "{sql}");
    }

    #[test]
    fn an_attrs_selection_filtered_further_keeps_the_plain_page() {
        let mut filtered = by_attrs(&["headSign"]);
        filtered.selection.q_expr = Some(parse("\"https://example.com/temperature\">1").expect("valid q"));
        let mut ordered = by_attrs(&["headSign"]);
        ordered.ordering.terms = vec![CompiledOrderTerm {
            target: OrderTarget::ModifiedAt,
            direction: OrderDirection::Desc,
            collation: None,
        }];
        for query in [filtered, ordered, by_attrs(&["neverWritten"])] {
            let built = build_query(&ContextTarget::Live, &query, &PageWindow::offset(100, 0), CountPolicy::Omit, &names()).expect("valid entity query");
            let sql = built.select.to_string(PostgresQueryBuilder);
            assert!(!sql.contains("short_walk") && !sql.contains("candidates"), "{sql}");
            assert_eq!(built.planning, StatementPlanning::Cached);
        }
    }
}
