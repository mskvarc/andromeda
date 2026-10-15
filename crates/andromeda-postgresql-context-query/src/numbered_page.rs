//! The page of a selection ordered by one Attribute whose every instance
//! holds a number (clause 4.23).
//!
//! The plain page reads each Entity's ordered value through a lateral and
//! sorts the whole selection on keys computed from it (the clause 4.23.2 rank
//! of the value's datatype, then the value as each datatype compares), which
//! no index orders, so a page of a hundred costs the selection. When every
//! instance of the Attribute holds a number, every Entity carrying it ranks as
//! a Number and compares on `value_number`, which the Attribute's value index
//! holds in order. The page is then three branches of a `UNION ALL`, each
//! behind a one-time gate:
//!
//! 1. the carriers: the instances of the Attribute in value order, each kept
//!    when it is its Entity's first standing instance in written order and its
//!    Entity is selected, ties broken by identifier. The planner walks the
//!    value index or sorts the selection's instances, whichever its estimates
//!    price lower, and reads as many as the window needs;
//! 2. the Entities without the Attribute, in identifier order, which clause
//!    4.23.2 ranks after every value, read only when the carriers do not fill
//!    the window;
//! 3. the plain page, when an instance of the Attribute holds another kind of
//!    value.
//!
//! Whether every instance holds a number is one probe of each other value
//! column's partial index, keyed by the Attribute. A walk in value order
//! probes each instance it visits for its Entity's earlier instances and its
//! selection, where the plain page reads each selected Entity once, so a
//! window ending past [`NUMBERED_BOUND`] keeps the plain page.

use crate::{
    attribute_value_datatype::{NUMBER_TYPE, datatype_rank},
    entity::QFilter,
    entity_page::{ProjectedPage, projected_page},
    gated_page::{PageSource, wrapped},
    one_time_gate::{count_of, materialized},
    result_ordering::{RankedValue, identifier_tiebreak, ranked_keys},
};
use andromeda_ngsi_ld_query::{
    count_policy::CountPolicy,
    entity::compiled::CompiledEntityQuery,
    entity_selection::ConsideredEntities,
    ordering::{sort_direction::SortDirection, term::OrderTarget},
    page_window::{PageStart, PageWindow},
    read_order::ReadOrder,
};
use andromeda_postgresql_query::{
    attribute_name_key::AttributeNameKey,
    dictionary::query_names::QueryNames,
    entity_ordering::ordering_collation,
    entity_row_refs::EntityRowRefs,
    error::RenderError,
    instance_expiry::instance_stands,
    q_predicate::projection_probe::{Ca, of_attribute, projection_column, standing},
    sort_key::SortKey,
};
use andromeda_postgresql_schema::current::CurrentAttributes;
use andromeda_postgresql_seaquery::{
    cast::typed_null,
    collation::{PostgresCollation, collate},
    literal::{int_literal, text_literal},
    pg_type::PostgresType,
};
use sea_query::{Alias, Asterisk, Condition, Expr, ExprTrait, Iden, JoinType, Query, SelectStatement, UnionType, WithClause};

/// The furthest a window may end, its offset and its limit together, for the
/// page to walk the value index rather than sort the selection.
pub const NUMBERED_BOUND: u64 = 10_000;

/// The CTE holding the carriers the window needs.
#[derive(Iden, Clone, Copy)]
#[iden = "numbered"]
struct Numbered;

/// The name an instance of the same Entity's Attribute written before the
/// carrier is read under.
#[derive(Iden, Clone, Copy)]
#[iden = "earlier"]
struct Earlier;

/// The value columns of an instance besides `value_number`.
///
/// An instance sets exactly one value column, so an Attribute with no instance
/// setting one of these holds numbers alone. Each has a partial index leading
/// on the Attribute's key, which answers the probe without reading a row.
const OTHER_VALUE_COLUMNS: [CurrentAttributes; 8] = [
    CurrentAttributes::ValueString,
    CurrentAttributes::ValueLongText,
    CurrentAttributes::ValueBoolean,
    CurrentAttributes::ValueGeometry,
    CurrentAttributes::ValueObject,
    CurrentAttributes::ValueDateTime,
    CurrentAttributes::ValueUri,
    CurrentAttributes::ValueLongUri,
];

/// An ordering the numbered page may answer: one term on the own value of an
/// Attribute the dictionary has recorded, over an offset window ending within
/// [`NUMBERED_BOUND`].
pub(crate) struct NumberedOrder {
    /// The ordered Attribute.
    attribute: AttributeNameKey,
    /// The direction the term runs in.
    direction: SortDirection,
    /// The collation the term compares text under, which the plain page's
    /// text key carries and every branch's must match.
    collation: PostgresCollation,
    /// How many Entities the window skips.
    offset: u32,
}

/// The numbered ordering `query` asks for over `window`, when the page may
/// answer it.
///
/// Only a selection by identifiers, types and `attrs` is paged this way: a
/// `q`, a geo-query or a scope query filters the Entities the walk visits, and
/// a selective one would have it visit most of the index for a page.
pub(crate) fn numbered_order(query: &CompiledEntityQuery, names: &QueryNames, window: &PageWindow) -> Option<NumberedOrder> {
    let offset = match window.start {
        PageStart::Offset(offset) if u64::from(offset) + u64::from(window.limit) <= NUMBERED_BOUND => offset,
        PageStart::Offset(_) | PageStart::Cursor(_) => return None,
    };
    let selection = &query.selection;
    let unfiltered = match selection.considered {
        ConsideredEntities::Selected => selection.q_expr.is_none() && selection.geo.is_none() && selection.scope_q.is_none(),
        ConsideredEntities::NoEntity => false,
    };
    let [term] = query.ordering.terms.as_slice() else {
        return None;
    };
    let path = match &term.target {
        OrderTarget::Attribute(path) => path,
        OrderTarget::Id | OrderTarget::Type | OrderTarget::Scope | OrderTarget::CreatedAt | OrderTarget::ModifiedAt => return None,
    };
    if !unfiltered || !path.is_simple() || term.direction.is_distance() {
        return None;
    }
    Some(NumberedOrder {
        attribute: names.attributes.get(&path.attr)?,
        direction: SortDirection::from(term.direction),
        collation: ordering_collation(term.collation.as_ref()),
        offset,
    })
}

/// The projected page of `source` ordered as `order` says, and the order its
/// rows come back in.
///
/// # Errors
///
/// Fails as rendering the plain page fails: a reference geometry with no EWKB
/// spelling, or a query-language rule a renderer decides first.
pub(crate) fn numbered_page(source: &PageSource<'_>, order: &NumberedOrder) -> Result<(ProjectedPage, ReadOrder), RenderError> {
    let need = source.need();
    let whole = source.condition(QFilter::Whole)?;
    let other_kind = other_kind(source.rows, order.attribute);
    // clone: every branch filters on the selection and reads the gate
    let carriers = carriers(source, order, whole.clone().add(other_kind.clone().not()), need);
    let without = without_attribute(
        source,
        order,
        whole.clone().add(other_kind.clone().not()).add(count_of(Numbered).lt(need)),
        need,
    );
    let (plain, read_order) = source.page(whole.add(other_kind), source.window)?;

    let mut ranked = Query::select();
    ranked.column(Asterisk).from(Numbered).union(UnionType::All, wrapped(without, "without"));
    let mut numbered = Query::select();
    numbered.column(Asterisk).from_subquery(ranked.take(), Alias::new("ranked"));
    for (column, key_order) in &plain.order {
        key_order.order_by(&mut numbered, Expr::col(column.clone())); // clone: both branches order by the same columns
    }
    numbered.limit(u64::from(source.window.limit)).offset(u64::from(order.offset));

    let mut union = wrapped(numbered.take(), "numbered_page");
    union
        .union(UnionType::All, wrapped(plain.select, "sorted"))
        .with_cte(WithClause::new().cte(materialized(carriers, Numbered)).to_owned());
    Ok((
        ProjectedPage {
            select: union.take(),
            order: plain.order,
        },
        read_order,
    ))
}

/// The first `need` carriers of the Attribute among the Entities `condition`
/// keeps, in the term's order, projected under the plain page's sort columns.
fn carriers(source: &PageSource<'_>, order: &NumberedOrder, condition: Condition, need: u64) -> SelectStatement {
    let rows = source.rows;
    let mut page = Query::select();
    source.relations.clone().apply_from(&mut page, source.schema);
    page.join(
        JoinType::InnerJoin,
        rows.table(CurrentAttributes::Table).alias(Ca),
        projection_column(CurrentAttributes::EntityKey).eq(rows.entity_key_column()),
    )
    .cond_where(
        condition
            .add(of_attribute(order.attribute))
            .add(projection_column(CurrentAttributes::ValueNumber).is_not_null())
            .add(standing())
            .add(Expr::exists(earlier_instance(rows, order.attribute)).not()),
    )
    .limit(need);
    let value = RankedValue {
        rank: datatype_rank(text_literal(NUMBER_TYPE), order.direction),
        number: projection_column(CurrentAttributes::ValueNumber),
        instant: typed_null(PostgresType::Timestamptz),
        text: collate(typed_null(PostgresType::Text), &order.collation),
    };
    projected_page(page, rows, keys(value, order, rows), CountPolicy::Omit).select
}

/// The first `need` Entities `condition` keeps that carry no standing instance
/// of the Attribute, in identifier order, projected under the plain page's
/// sort columns with no value, which clause 4.23.2 ranks last.
fn without_attribute(source: &PageSource<'_>, order: &NumberedOrder, condition: Condition, need: u64) -> SelectStatement {
    let rows = source.rows;
    let mut carried = Query::select();
    carried
        .expr(int_literal(1))
        .from(rows.table(CurrentAttributes::Table).alias(Ca))
        .and_where(projection_column(CurrentAttributes::EntityKey).eq(rows.entity_key_column()))
        .and_where(of_attribute(order.attribute))
        .and_where(standing());
    let mut page = Query::select();
    source.relations.clone().apply_from(&mut page, source.schema);
    page.cond_where(condition.add(Expr::exists(carried.take()).not())).limit(need);
    let value = RankedValue {
        rank: typed_null(PostgresType::Integer),
        number: typed_null(PostgresType::Numeric),
        instant: typed_null(PostgresType::Timestamptz),
        text: collate(typed_null(PostgresType::Text), &order.collation),
    };
    projected_page(page, rows, keys(value, order, rows), CountPolicy::Omit).select
}

/// The keys a branch orders `value` by: the term's four, then the identifier.
fn keys(value: RankedValue, order: &NumberedOrder, rows: &EntityRowRefs) -> Vec<SortKey> {
    ranked_keys(value, order.direction).into_iter().chain([identifier_tiebreak(rows)]).collect()
}

/// A standing instance of the carrier's Entity's Attribute written before it:
/// the ordered value is the first standing instance's in written order.
fn earlier_instance(rows: &EntityRowRefs, attribute: AttributeNameKey) -> SelectStatement {
    let column = |column: CurrentAttributes| Expr::col((Earlier, column));
    Query::select()
        .expr(int_literal(1))
        .from(rows.table(CurrentAttributes::Table).alias(Earlier))
        .and_where(column(CurrentAttributes::EntityKey).eq(projection_column(CurrentAttributes::EntityKey)))
        .and_where(column(CurrentAttributes::AttributeNameKey).eq(attribute.get()))
        .and_where(instance_stands(column(CurrentAttributes::ExpiresAt)))
        .and_where(column(CurrentAttributes::Ordinal).lt(projection_column(CurrentAttributes::Ordinal)))
        .take()
}

/// Whether an instance of the Attribute, standing or not, holds a value other
/// than a number.
///
/// A disjunction kept as its own condition, so that beside the selection's
/// filters it is parenthesised as one term.
fn other_kind(rows: &EntityRowRefs, attribute: AttributeNameKey) -> Condition {
    OTHER_VALUE_COLUMNS.into_iter().fold(Condition::any(), |kinds, column| {
        let mut holder = Query::select();
        holder
            .expr(int_literal(1))
            .from(rows.table(CurrentAttributes::Table).alias(Ca))
            .and_where(of_attribute(attribute))
            .and_where(projection_column(column).is_not_null());
        kinds.add(Expr::exists(holder.take()))
    })
}

#[cfg(test)]
mod tests {
    use crate::{entity::build_query, entity_total::EntityTotal};
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_geometry::geometry::Geometry;
    use andromeda_ngsi_ld_query::{
        count_policy::CountPolicy,
        entity::compiled::CompiledEntityQuery,
        entity_selection::{EntitySelection, EntitySelector},
        filter::{parser::parse, path::AttrPath},
        ordering::{
            geometry::OrderingGeometry,
            term::{CompiledOrderTerm, OrderDirection, OrderTarget},
        },
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
    use sea_query::PostgresQueryBuilder;

    const SPEED: &str = "https://example.com/speed";

    /// `speed` at key 7 and `location` at key 9; the type `Vehicle` at key 1.
    fn names() -> QueryNames {
        QueryNames {
            attributes: ResolvedNames::from_pairs(
                [("speed", 7), ("location", 9)]
                    .map(|(name, key)| (format!("https://example.com/{name}").parse().expect("valid name"), AttributeNameKey::from(key))),
            ),
            types: ResolvedNames::from_pairs([("https://example.com/Vehicle".parse().expect("valid type name"), EntityTypeNameKey::from(1))]),
        }
    }

    fn term(attribute: &str, direction: OrderDirection) -> CompiledOrderTerm {
        CompiledOrderTerm {
            target: OrderTarget::Attribute(AttrPath::simple(attribute.parse().expect("valid attribute name"))),
            direction,
            collation: None,
        }
    }

    fn vehicles_by(terms: Vec<CompiledOrderTerm>) -> CompiledEntityQuery {
        let mut query = CompiledEntityQuery::builder().selection(EntitySelection::builder().build()).build();
        query.selection.selectors = vec![
            EntitySelector::builder()
                .type_expr(Some(TypeExpr::Type(IriBuf::new("https://example.com/Vehicle".to_owned()).expect("valid IRI"))))
                .build(),
        ];
        query.ordering.terms = terms;
        query
    }

    fn sql(query: &CompiledEntityQuery, window: &PageWindow) -> String {
        build_query(&ContextTarget::Live, query, window, CountPolicy::Omit, &names())
            .expect("valid entity query")
            .select
            .to_string(PostgresQueryBuilder)
    }

    /// The carriers' CTE, the branch of the Entities without the Attribute,
    /// and the plain page, in the order the statement holds them.
    fn branches(sql: &str) -> (&str, &str, &str) {
        let (numbered, rest) = sql
            .split_once(r#") SELECT * FROM (SELECT * FROM (SELECT * FROM "numbered" UNION ALL"#)
            .unwrap_or_else(|| panic!("a numbered page: {sql}"));
        let (without, plain) = rest
            .split_once(r#"AS "numbered_page" UNION ALL"#)
            .unwrap_or_else(|| panic!("the plain page follows: {sql}"));
        (numbered, without, plain)
    }

    /// The order the head statement restates on the page's columns.
    fn head_order(sql: &str) -> &str {
        sql.rsplit_once(r#"AS "stored" ON TRUE "#).map_or(sql, |(_, order)| order)
    }

    #[test]
    fn a_number_ordering_reads_the_carriers_in_value_order_behind_a_numbers_only_gate() {
        let sql = sql(&vehicles_by(vec![term(SPEED, OrderDirection::Asc)]), &PageWindow::offset(100, 0));
        let (numbered, _, _) = branches(&sql);
        assert!(
            numbered.contains(concat!(
                r#""ca"."value_number" AS "sort_1", NULL::timestamptz AS "sort_2", (NULL::text) COLLATE "und-x-icu" AS "sort_3", "selected"."id" AS "sort_4" "#,
                r#"FROM (SELECT "entity_key", "id" FROM "entity_type_members" WHERE "entity_type_name_key" = (1)) AS "selected" "#,
                r#"INNER JOIN "current_attributes" AS "ca" ON "ca"."entity_key" = "selected"."entity_key""#
            )),
            "{numbered}"
        );
        assert!(
            numbered.contains(r#""ca"."attribute_name_key" = 7 AND "ca"."value_number" IS NOT NULL"#),
            "{numbered}"
        );
        for column in [
            "value_string",
            "value_long_text",
            "value_boolean",
            "value_geometry",
            "value_object",
            "value_date_time",
            "value_uri",
            "value_long_uri",
        ] {
            assert!(
                numbered.contains(&format!(r#""ca"."attribute_name_key" = 7 AND "ca"."{column}" IS NOT NULL"#)),
                "the gate probes {column}: {numbered}"
            );
        }
        assert!(
            numbered.ends_with(r#"ORDER BY "sort_0" ASC NULLS LAST, "sort_1" ASC, "sort_2" ASC, "sort_3" ASC, "sort_4" ASC LIMIT 100"#),
            "the carriers read as many as the window needs: {numbered}"
        );
        assert_eq!(sql.matches("UNION ALL").count(), 2, "{sql}");
    }

    /// Clause 4.23: an Entity orders by the first standing instance of the
    /// Attribute in written order, so a carrier written after another
    /// standing instance of its Entity is not the one it orders by.
    #[test]
    fn a_carrier_is_its_entitys_first_standing_instance_in_written_order() {
        let sql = sql(&vehicles_by(vec![term(SPEED, OrderDirection::Asc)]), &PageWindow::offset(100, 0));
        let (numbered, _, _) = branches(&sql);
        assert!(
            numbered.contains(concat!(
                r#"("ca"."expires_at" IS NULL OR "ca"."expires_at" > NOW()) AND (NOT EXISTS(SELECT 1 FROM "current_attributes" AS "earlier" "#,
                r#"WHERE "earlier"."entity_key" = "ca"."entity_key" AND "earlier"."attribute_name_key" = 7 "#,
                r#"AND ("earlier"."expires_at" IS NULL OR "earlier"."expires_at" > NOW()) AND "earlier"."ordinal" < "ca"."ordinal"))"#
            )),
            "{numbered}"
        );
    }

    /// Clause 4.23.2 ranks an Entity without the Attribute after every value,
    /// so those are read in identifier order once the carriers fall short.
    #[test]
    fn entities_without_the_attribute_follow_only_when_the_carriers_fall_short() {
        let sql = sql(&vehicles_by(vec![term(SPEED, OrderDirection::Asc)]), &PageWindow::offset(20, 40));
        let (_, without, _) = branches(&sql);
        assert!(
            without.contains(r#"SELECT "selected"."entity_key", NULL::integer AS "sort_0", NULL::numeric AS "sort_1", NULL::timestamptz AS "sort_2""#),
            "{without}"
        );
        assert!(without.contains(r#"(SELECT COUNT(*) FROM "numbered") < 60"#), "{without}");
        assert!(
            without.contains(concat!(
                r#"(NOT EXISTS(SELECT 1 FROM "current_attributes" AS "ca" WHERE "ca"."entity_key" = "selected"."entity_key" "#,
                r#"AND "ca"."attribute_name_key" = 7 AND ("ca"."expires_at" IS NULL OR "ca"."expires_at" > NOW())))"#
            )),
            "{without}"
        );
        assert!(
            without.contains(r#""sort_4" ASC LIMIT 60) AS "without")) AS "ranked" ORDER BY "sort_0" ASC NULLS LAST"#),
            "{without}"
        );
        assert!(
            without.ends_with(r#""sort_4" ASC LIMIT 20 OFFSET 40) "#),
            "the window is cut over both branches: {without}"
        );
    }

    #[test]
    fn the_plain_page_answers_when_an_instance_holds_another_kind() {
        let sql = sql(&vehicles_by(vec![term(SPEED, OrderDirection::Asc)]), &PageWindow::offset(100, 0));
        let (_, _, plain) = branches(&sql);
        assert!(plain.contains(r#"LEFT JOIN LATERAL (SELECT COALESCE("ordered"."value_number""#), "{plain}");
        assert!(
            plain.contains(
                r#"WHERE EXISTS(SELECT 1 FROM "current_attributes" AS "ca" WHERE "ca"."attribute_name_key" = 7 AND "ca"."value_string" IS NOT NULL) OR "#
            ),
            "{plain}"
        );
        assert!(plain.contains(r#"LIMIT 100 OFFSET 0) AS "sorted""#), "{plain}");
    }

    /// The gate is a disjunction, which beside the selection's own filters
    /// must stay one term.
    #[test]
    fn the_gate_is_one_term_beside_the_selections_other_filters() {
        let mut query = vehicles_by(vec![term(SPEED, OrderDirection::Asc)]);
        query.selection.attrs = vec![IriBuf::new(SPEED.to_owned()).expect("valid IRI")];
        let sql = sql(&query, &PageWindow::offset(100, 0));
        let (numbered, without, plain) = branches(&sql);
        assert!(
            plain.contains(
                r#") AND (EXISTS(SELECT 1 FROM "current_attributes" AS "ca" WHERE "ca"."attribute_name_key" = 7 AND "ca"."value_string" IS NOT NULL) OR "#
            ),
            "{plain}"
        );
        for branch in [numbered, without] {
            assert!(
                branch.contains(r#") AND (NOT (EXISTS(SELECT 1 FROM "current_attributes" AS "ca" WHERE "ca"."attribute_name_key" = 7 AND "ca"."value_string" IS NOT NULL) OR "#),
                "{branch}"
            );
        }
    }

    /// Clause 4.23.2 reverses the datatype order for a descending term and
    /// keeps the absent Attribute last; the identifier breaks ties ascending
    /// either way (clause 5.5.9).
    #[test]
    fn a_descending_number_ordering_ranks_numbers_in_the_reversed_sequence_and_keeps_the_identifier_ascending() {
        let sql = sql(&vehicles_by(vec![term(SPEED, OrderDirection::Desc)]), &PageWindow::offset(100, 0));
        let (numbered, _, _) = branches(&sql);
        assert!(numbered.contains(r#"'string','number','null']::text[], 'number') AS "sort_0""#), "{numbered}");
        assert!(
            numbered.ends_with(r#"ORDER BY "sort_0" ASC NULLS LAST, "sort_1" DESC, "sort_2" DESC, "sort_3" DESC, "sort_4" ASC LIMIT 100"#),
            "{numbered}"
        );
    }

    /// Every branch projects the columns the plain page sorts on, so the head
    /// statement restates one order whichever branch answered.
    #[test]
    fn a_numbered_page_restates_the_order_of_the_plain_page() {
        for direction in [OrderDirection::Asc, OrderDirection::Desc] {
            let query = vehicles_by(vec![term(SPEED, direction)]);
            let numbered = sql(&query, &PageWindow::offset(100, 0));
            let plain = sql(&query, &PageWindow::offset(100, 20_000));
            assert!(numbered.contains(r#"WITH "numbered""#), "{numbered}");
            assert!(!plain.contains(r#""numbered""#), "{plain}");
            assert_eq!(head_order(&numbered), head_order(&plain));
        }
    }

    #[test]
    fn a_window_ending_past_the_bound_keeps_the_plain_page() {
        let query = vehicles_by(vec![term(SPEED, OrderDirection::Asc)]);
        assert!(sql(&query, &PageWindow::offset(100, 9_900)).contains(r#"WITH "numbered""#));
        assert!(!sql(&query, &PageWindow::offset(100, 9_901)).contains(r#""numbered""#));
    }

    #[test]
    fn an_unfiltered_selection_reads_the_carriers_beside_the_entities() {
        let mut query = vehicles_by(vec![term(SPEED, OrderDirection::Asc)]);
        query.selection.selectors = vec![EntitySelector::builder().build()];
        let sql = sql(&query, &PageWindow::offset(100, 0));
        let (numbered, _, _) = branches(&sql);
        assert!(
            numbered.contains(r#"FROM "entities" INNER JOIN "current_attributes" AS "ca" ON "ca"."entity_key" = "entities"."entity_key""#),
            "{numbered}"
        );
    }

    #[test]
    fn an_ordering_the_value_index_cannot_serve_keeps_the_plain_page() {
        let mut filtered = vehicles_by(vec![term(SPEED, OrderDirection::Asc)]);
        filtered.selection.q_expr = Some(parse("\"https://example.com/speed\">1").expect("valid q"));
        let two_terms = vehicles_by(vec![term(SPEED, OrderDirection::Asc), term(SPEED, OrderDirection::Desc)]);
        let mut sub_attribute = vehicles_by(vec![term(SPEED, OrderDirection::Asc)]);
        if let Some(CompiledOrderTerm {
            target: OrderTarget::Attribute(path),
            ..
        }) = sub_attribute.ordering.terms.first_mut()
        {
            path.sub_attrs = vec!["https://example.com/accuracy".parse().expect("valid attribute name")];
        }
        let mut distance = vehicles_by(vec![term("https://example.com/location", OrderDirection::DistAsc)]);
        distance.ordering.geometry = Some(OrderingGeometry::new(
            serde_json::from_value::<Geometry>(serde_json::json!({ "type": "Point", "coordinates": [13.4, 52.5] })).expect("point parses"),
        ));
        let unrecorded = vehicles_by(vec![term("https://example.com/neverWritten", OrderDirection::Asc)]);
        let mut by_identifier = vehicles_by(vec![]);
        by_identifier.ordering.terms = vec![CompiledOrderTerm {
            target: OrderTarget::Id,
            direction: OrderDirection::Asc,
            collation: None,
        }];
        for query in [filtered, two_terms, sub_attribute, distance, unrecorded, by_identifier] {
            let built = build_query(&ContextTarget::Live, &query, &PageWindow::offset(100, 0), CountPolicy::Omit, &names()).expect("valid entity query");
            let sql = built.select.to_string(PostgresQueryBuilder);
            assert!(!sql.contains(r#""numbered""#), "{sql}");
            assert_eq!(built.planning, StatementPlanning::Cached);
        }
    }

    /// The carriers' index is priced only with the window's bound values, and
    /// the plain branch, gated off, must launch no parallel worker.
    #[test]
    fn a_numbered_page_is_planned_with_its_bound_values_and_runs_serially() {
        let built = build_query(
            &ContextTarget::Live,
            &vehicles_by(vec![term(SPEED, OrderDirection::Asc)]),
            &PageWindow::offset(100, 0),
            CountPolicy::Omit,
            &names(),
        )
        .expect("valid entity query");
        assert_eq!(built.planning, StatementPlanning::GatedBranches);
        assert_eq!(built.total_planning, StatementPlanning::Cached);
    }

    /// Clause 6.3.13: the carriers' branch reads part of the selection, so
    /// the total is its own count over the whole selection.
    #[test]
    fn a_counted_numbered_page_counts_the_selection_on_its_own() {
        let built = build_query(
            &ContextTarget::Live,
            &vehicles_by(vec![term(SPEED, OrderDirection::Asc)]),
            &PageWindow::offset(100, 0),
            CountPolicy::Emit,
            &names(),
        )
        .expect("valid entity query");
        let EntityTotal::Counted(count) = built.total else {
            panic!("a numbered page carries no total on its rows");
        };
        let count = count.to_string(PostgresQueryBuilder);
        assert!(count.starts_with("SELECT COUNT(*) FROM "), "{count}");
        assert!(!count.contains("numbered") && !count.contains("current_attributes"), "{count}");
        assert!(!built.select.to_string(PostgresQueryBuilder).contains("total_count"));
    }
}
