//! The page of a selection ordered by one Attribute whose every instance
//! holds a number, or whose every instance holds a `DateTime` (clause 4.23).
//!
//! The plain page reads each Entity's ordered value through a lateral and
//! sorts the whole selection on keys computed from it (the clause 4.23.2 rank
//! of the value's datatype, then the value as each datatype compares), which
//! no index orders, so a page of a hundred costs the selection. When every
//! instance of the Attribute holds a number, every Entity carrying it ranks as
//! a Number and compares on `value_number`; when every instance holds a
//! `DateTime`, every carrier ranks as a `DateTime` and compares on the instant
//! in `value_date_time` (clause 4.23.1). The Attribute's value index holds
//! either in order. The page is then a `UNION ALL` of branches, each behind a
//! one-time gate on the columns the Attribute's instances occupy:
//!
//! 1. the number walk and the `DateTime` walk: the instances of the Attribute
//!    in value order, each kept when it is its Entity's first standing
//!    instance in written order and its Entity is selected, ties broken by
//!    identifier. Each is gated on its column holding every instance, so at
//!    most one reads anything. The planner walks the value index or sorts the
//!    selection's instances, whichever its estimates price lower, and reads as
//!    many as the window needs;
//! 2. the Entities without the Attribute, in identifier order, which clause
//!    4.23.2 ranks after every value, read only when a walk's gate opened and
//!    the walks do not fill the window;
//! 3. the plain page, when neither gate opened.
//!
//! The statement decides at run time which branch answers, so an Attribute
//! with no instance left opens both gates: the walks read nothing and the
//! Entities without it fill the page. A walk in value order probes each
//! instance it visits for its Entity's earlier instances and its selection,
//! where the plain page reads each selected Entity once, so a window ending
//! past [`VALUE_WALK_BOUND`] keeps the plain page.

use crate::{
    entity::QFilter,
    entity_page::{ProjectedPage, projected_page},
    gated_page::PageSource,
    occupied_columns::{OccupiedColumns, held_alone, occupied_columns},
    result_ordering::{RankedValue, identifier_tiebreak, ranked_keys},
    walked_column::{WalkedCarriers, WalkedColumn},
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
    expiry::unexpired,
    one_time_gate::{count_of, materialized, wrapped},
    q_predicate::projection_probe::{Ca, of_attribute, projection_column, standing},
    sort_key::SortKey,
};
use andromeda_postgresql_schema::current::CurrentAttributes;
use andromeda_postgresql_seaquery::{
    cast::typed_null,
    collation::{PostgresCollation, collate},
    literal::int_literal,
    pg_type::PostgresType,
};
use sea_query::{Alias, Asterisk, Condition, Expr, ExprTrait, Iden, JoinType, Query, SelectStatement, UnionType, WithClause};

/// The furthest row a walk reads, the window's offset and its limit together,
/// for the page to walk the value index rather than sort the selection.
///
/// A client window ending at 10,000 reads one row more, the one whose
/// presence tells whether the collection continues (clause 5.5.9), so the
/// bound counts it.
pub const VALUE_WALK_BOUND: u64 = 10_001;

/// The name an instance of the same Entity's Attribute written before the
/// carrier is read under.
#[derive(Iden, Clone, Copy)]
#[iden = "earlier"]
struct Earlier;

/// An ordering the value walk page may answer: one term on the own value of
/// an Attribute the dictionary has recorded, over an offset window ending
/// within [`VALUE_WALK_BOUND`].
pub(crate) struct ValueWalkOrder {
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

/// The ordering `query` asks for over `window`, when the value walk page may
/// answer it.
///
/// Only a selection by identifiers, types and `attrs` is paged this way: a
/// `q`, a geo-query or a scope query filters the Entities the walk visits, and
/// a selective one would have it visit most of the index for a page.
pub(crate) fn value_walk_order(query: &CompiledEntityQuery, names: &QueryNames, window: &PageWindow) -> Option<ValueWalkOrder> {
    let offset = match window.start {
        PageStart::Offset(offset) if u64::from(offset) + u64::from(window.limit) <= VALUE_WALK_BOUND => offset,
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
    Some(ValueWalkOrder {
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
pub(crate) fn value_walk_page(source: &PageSource<'_>, order: &ValueWalkOrder) -> Result<(ProjectedPage, ReadOrder), RenderError> {
    let need = source.need();
    let whole = source.condition(QFilter::Whole)?;
    let mut ctes = WithClause::new();
    ctes.cte(materialized(occupied_columns(source.rows, order.attribute), OccupiedColumns));
    let mut walked = Condition::any();
    let mut sorted = whole.clone();
    for column in WalkedColumn::ALL {
        let gate = held_alone(column.held());
        let carriers = carriers(source, order, column, whole.clone().add(gate.clone()), need);
        ctes.cte(materialized(carriers, column.carriers()));
        walked = walked.add(gate.clone());
        sorted = sorted.add(gate.not());
    }
    let carried = count_of(WalkedCarriers::Numbers).add(count_of(WalkedCarriers::Instants));
    let without = without_attribute(source, order, whole.add(walked).add(carried.lt(need)), need);
    let (plain, read_order) = source.page(sorted, source.window)?;

    let mut ranked = Query::select();
    ranked
        .column(Asterisk)
        .from(WalkedCarriers::Numbers)
        .union(UnionType::All, Query::select().column(Asterisk).from(WalkedCarriers::Instants).take())
        .union(UnionType::All, wrapped(without, "without"));

    let mut walk = Query::select();
    walk.column(Asterisk).from_subquery(ranked.take(), Alias::new("ranked"));
    for (column, key_order) in &plain.order {
        key_order.order_by(&mut walk, Expr::col(column.clone()));
    }
    walk.limit(u64::from(source.window.limit)).offset(u64::from(order.offset));

    let mut union = wrapped(walk.take(), "value_walk");
    union.union(UnionType::All, wrapped(plain.select, "sorted")).with_cte(ctes);
    Ok((
        ProjectedPage {
            select: union.take(),
            order: plain.order,
        },
        read_order,
    ))
}

/// The first `need` carriers of the Attribute in `column` among the Entities
/// `condition` keeps, in the term's order, projected under the plain page's
/// sort columns.
fn carriers(source: &PageSource<'_>, order: &ValueWalkOrder, column: WalkedColumn, condition: Condition, need: u64) -> SelectStatement {
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
            .add(projection_column(column.held().column()).is_not_null())
            .add(standing())
            .add(Expr::exists(earlier_instance(rows, order.attribute)).not()),
    )
    .limit(need);
    let value = column.ranked_value(order.direction, &order.collation);
    projected_page(page, rows, keys(value, order, rows), CountPolicy::Omit).select
}

/// The first `need` Entities `condition` keeps that carry no standing instance
/// of the Attribute, in identifier order, projected under the plain page's
/// sort columns with no value, which clause 4.23.2 ranks last.
fn without_attribute(source: &PageSource<'_>, order: &ValueWalkOrder, condition: Condition, need: u64) -> SelectStatement {
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
fn keys(value: RankedValue, order: &ValueWalkOrder, rows: &EntityRowRefs) -> Vec<SortKey> {
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
        .and_where(unexpired(column(CurrentAttributes::ExpiresAt)))
        .and_where(column(CurrentAttributes::Ordinal).lt(projection_column(CurrentAttributes::Ordinal)))
        .take()
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
    use andromeda_postgresql_seaquery::{assert_statement_snapshot, statement_planning::StatementPlanning};
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, SelectStatement};

    const SPEED: &str = "https://example.com/speed";

    /// `speed` at key 7 and `location` at key 9; the type `Vehicle` at key 1.
    fn names() -> QueryNames {
        QueryNames {
            attributes: ResolvedNames::from_pairs(
                [("speed", 7), ("location", 9)]
                    .map(|(name, key)| (format!("https://example.com/{name}").parse().expect("valid name"), AttributeNameKey::from(key))),
            ),
            types: ResolvedNames::from_pairs([("https://example.com/Vehicle".parse().expect("valid type name"), EntityTypeNameKey::from(1))]),
            ..QueryNames::default()
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

    fn statement(query: &CompiledEntityQuery, window: &PageWindow) -> SelectStatement {
        build_query(&ContextTarget::Live, query, window, CountPolicy::Omit, &names())
            .expect("valid entity query")
            .select
    }

    /// The order the head statement restates on the page's columns.
    fn head_order(select: &SelectStatement) -> String {
        let sql = select.to_string(PostgresQueryBuilder);
        sql.rsplit_once(r#"AS "stored" ON TRUE "#).map_or(sql.as_str(), |(_, order)| order).to_owned()
    }

    #[test]
    fn a_number_ordering_reads_the_carriers_in_value_order() {
        assert_statement_snapshot!(
            "value-walk-page-a-number-ordering-reads-the-carriers-in-value-order",
            statement(&vehicles_by(vec![term(SPEED, OrderDirection::Asc)]), &PageWindow::offset(100, 0))
        );
    }

    /// Clause 4.23.1: a `DateTime` compares as its instant.
    #[test]
    fn a_date_time_ordering_reads_the_carriers_in_instant_order() {
        assert_statement_snapshot!(
            "value-walk-page-a-date-time-ordering-reads-the-carriers-in-instant-order",
            statement(&vehicles_by(vec![term(SPEED, OrderDirection::Asc)]), &PageWindow::offset(100, 0))
        );
    }

    #[test]
    fn each_walk_is_gated_on_its_column_held_alone() {
        assert_statement_snapshot!(
            "value-walk-page-each-walk-is-gated-on-its-column-held-alone",
            statement(&vehicles_by(vec![term(SPEED, OrderDirection::Asc)]), &PageWindow::offset(100, 0))
        );
    }

    #[test]
    fn the_occupied_columns_are_read_once() {
        assert_statement_snapshot!(
            "value-walk-page-the-occupied-columns-are-read-once",
            statement(&vehicles_by(vec![term(SPEED, OrderDirection::Asc)]), &PageWindow::offset(100, 0))
        );
    }

    /// Clause 4.23: an Entity orders by the first standing instance of the
    /// Attribute in written order, so a carrier written after another
    /// standing instance of its Entity is not the one it orders by.
    #[test]
    fn a_carrier_is_its_entitys_first_standing_instance_in_written_order() {
        assert_statement_snapshot!(
            "value-walk-page-a-carrier-is-its-entitys-first-standing-instance-in-written-order",
            statement(&vehicles_by(vec![term(SPEED, OrderDirection::Asc)]), &PageWindow::offset(100, 0))
        );
    }

    /// Clause 4.23.2 ranks an Entity without the Attribute after every value,
    /// so those are read in identifier order once the walks fall short.
    #[test]
    fn entities_without_the_attribute_follow_only_when_both_walks_fall_short() {
        assert_statement_snapshot!(
            "value-walk-page-entities-without-the-attribute-follow-only-when-both-walks-fall-short",
            statement(&vehicles_by(vec![term(SPEED, OrderDirection::Asc)]), &PageWindow::offset(20, 40))
        );
    }

    #[test]
    fn the_plain_page_answers_when_neither_column_is_held_alone() {
        assert_statement_snapshot!(
            "value-walk-page-the-plain-page-answers-when-neither-column-is-held-alone",
            statement(&vehicles_by(vec![term(SPEED, OrderDirection::Asc)]), &PageWindow::offset(100, 0))
        );
    }

    /// Each gate beside the selection's own filters must stay one term.
    #[test]
    fn the_gate_is_one_term_beside_the_selections_other_filters() {
        let mut query = vehicles_by(vec![term(SPEED, OrderDirection::Asc)]);
        query.selection.attrs = vec![IriBuf::new(SPEED.to_owned()).expect("valid IRI")];
        assert_statement_snapshot!(
            "value-walk-page-the-gate-is-one-term-beside-the-selections-other-filters",
            statement(&query, &PageWindow::offset(100, 0))
        );
    }

    /// Clause 4.23.2 reverses the datatype order for a descending term and
    /// keeps the absent Attribute last; the identifier breaks ties ascending
    /// either way (clause 5.5.9).
    #[test]
    fn a_descending_number_ordering_ranks_numbers_in_the_reversed_sequence_and_keeps_the_identifier_ascending() {
        assert_statement_snapshot!(
            "value-walk-page-a-descending-number-ordering-ranks-numbers-in-the-reversed-sequence-and-keeps-the-identifier-ascending",
            statement(&vehicles_by(vec![term(SPEED, OrderDirection::Desc)]), &PageWindow::offset(100, 0))
        );
    }

    /// Clause 4.23.2 reverses the datatype order for a descending term, which
    /// puts `DateTime` first, and keeps the absent Attribute last; the
    /// identifier breaks ties ascending either way (clause 5.5.9).
    #[test]
    fn a_descending_date_time_ordering_ranks_date_times_first_and_keeps_the_identifier_ascending() {
        assert_statement_snapshot!(
            "value-walk-page-a-descending-date-time-ordering-ranks-date-times-first-and-keeps-the-identifier-ascending",
            statement(&vehicles_by(vec![term(SPEED, OrderDirection::Desc)]), &PageWindow::offset(100, 0))
        );
    }

    /// Every branch projects the columns the plain page sorts on, so the head
    /// statement restates one order whichever branch answered.
    #[test]
    fn a_value_walk_restates_the_order_of_the_plain_page() {
        for (index, direction) in [OrderDirection::Asc, OrderDirection::Desc].into_iter().enumerate() {
            let query = vehicles_by(vec![term(SPEED, direction)]);
            let walked = statement(&query, &PageWindow::offset(100, 0));
            assert_statement_snapshot!(
                format!("value-walk-page-a-value-walk-restates-the-order-of-the-plain-page-{}", index * 2 + 1),
                walked
            );
            let plain = statement(&query, &PageWindow::offset(100, 20_000));
            assert_statement_snapshot!(
                format!("value-walk-page-a-value-walk-restates-the-order-of-the-plain-page-{}", index * 2 + 2),
                plain
            );
            assert_eq!(head_order(&walked), head_order(&plain));
        }
    }

    /// A client window reads one row past its end (clause 5.5.9), so the
    /// window a client asks for ending at 10,000 is walked.
    #[test]
    fn a_client_window_ending_at_the_bound_is_walked() {
        let query = vehicles_by(vec![term(SPEED, OrderDirection::Asc)]);
        assert_statement_snapshot!(
            "value-walk-page-a-client-window-ending-at-the-bound-is-walked-1",
            statement(&query, &PageWindow::offset(100, 9_900).with_lookahead())
        );
        assert_statement_snapshot!(
            "value-walk-page-a-client-window-ending-at-the-bound-is-walked-2",
            statement(&query, &PageWindow::offset(100, 9_901).with_lookahead())
        );
    }

    #[test]
    fn an_unfiltered_selection_reads_the_carriers_beside_the_entities() {
        let mut query = vehicles_by(vec![term(SPEED, OrderDirection::Asc)]);
        query.selection.selectors = vec![EntitySelector::builder().build()];
        assert_statement_snapshot!(
            "value-walk-page-an-unfiltered-selection-reads-the-carriers-beside-the-entities",
            statement(&query, &PageWindow::offset(100, 0))
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
        for (index, query) in [filtered, two_terms, sub_attribute, distance, unrecorded, by_identifier]
            .into_iter()
            .enumerate()
        {
            let built = build_query(&ContextTarget::Live, &query, &PageWindow::offset(100, 0), CountPolicy::Omit, &names()).expect("valid entity query");
            assert_statement_snapshot!(
                format!("value-walk-page-an-ordering-the-value-index-cannot-serve-keeps-the-plain-page-{}", index + 1),
                built.select
            );
            assert_eq!(built.planning, StatementPlanning::Cached);
        }
    }

    /// The walks' indexes are priced only with the window's bound values, and
    /// the plain branch, gated off, must launch no parallel worker.
    #[test]
    fn a_value_walk_is_planned_with_its_bound_values_and_runs_serially() {
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

    /// Clause 6.3.13: a walk reads part of the selection, so the total is its
    /// own count over the whole selection.
    #[test]
    fn a_counted_value_walk_counts_the_selection_on_its_own() {
        let built = build_query(
            &ContextTarget::Live,
            &vehicles_by(vec![term(SPEED, OrderDirection::Asc)]),
            &PageWindow::offset(100, 0),
            CountPolicy::Emit,
            &names(),
        )
        .expect("valid entity query");
        let EntityTotal::Counted(count) = built.total else {
            panic!("a value walk page carries no total on its rows");
        };
        assert_statement_snapshot!("value-walk-page-a-counted-value-walk-counts-the-selection-on-its-own-1", count.count);
        assert_statement_snapshot!("value-walk-page-a-counted-value-walk-counts-the-selection-on-its-own-2", built.select);
    }
}
