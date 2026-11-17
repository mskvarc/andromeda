//! The page of a selection by `attrs` alone, which keeps only the Entities
//! that carry at least one of the Attributes (clause 5.7.2.4).
//!
//! The carriers of an Attribute are one key of the GIN over the search row's
//! names, which yields them in no order, while the page wants them in
//! identifier order (clause 5.5.9). The planner assumes the carriers spread
//! evenly through `uq_entities_id`, so for an Attribute with more than a few
//! thousand carriers it walks that index and filters each Entity; when they
//! sort late, as the Entities of one type do behind those of a larger one, it
//! visits every Entity before them. The page is therefore
//! a [`gated_page`]:
//!
//! 1. the short walk visits the first [`CARRIER_SHORT_WALK`] Entities in
//!    identifier order, each probed through its search row;
//! 2. the index reads at most [`CARRIER_BOUND`] + 1 carriers, and the page is
//!    cut from them, sorted;
//! 3. the full walk probes every Entity in identifier order through its
//!    search row.
//!
//! Only a page in identifier order is cut this way. Under an `orderBy` the
//! plain page sorts the selection whichever relation it reads first, so the
//! walk is never the planner's choice for it.

use crate::{
    carried_names::attrs_carriers,
    entity::{GeoFilter, QFilter, build_entity_conditions},
    entity_page::{ProjectedPage, StepColumn},
    entity_relations::{Demand, EntityRelations, RowSource},
    gated_page::{GatedColumns, IndexRead, PageSource, PageWalk, Walked, gated_page},
};
use andromeda_ngsi_ld_query::{count_policy::CountPolicy, entity::compiled::CompiledEntityQuery, entity_selection::ConsideredEntities, read_order::ReadOrder};
use andromeda_postgresql_query::{entity_row_refs::EntityRowRefs, error::RenderError, one_time_gate::CandidateBound, query_names::QueryNames};
use andromeda_postgresql_schema::current::EntitySearch;
use sea_query::{Condition, DynIden, Expr, ExprTrait, Iden, Query, SelectStatement};

/// How many Entities the short walk visits before the carriers are read.
///
/// A visit costs one primary-key probe of the search row, a quarter to half a
/// microsecond, so the walk is longer than a keyed page's, whose visits each
/// probe the Entity's attribute rows. A walk that falls short costs 1 to 3 ms
/// before the carriers are read. An Attribute one Entity in two hundred carries
/// fills a page of twenty within it, among 1 000 000 Entities in about 8 ms
/// where reading its carriers costs 33 ms.
pub const CARRIER_SHORT_WALK: u32 = 5_000;

/// The most carriers the index step reads and sorts.
///
/// A carrier costs 2 to 6 µs to read, narrow and sort, so the bound's
/// carriers cost over 200 ms. An Attribute spread through the selection with
/// that many carriers fills the page within the short walk, so the carriers
/// are read in full only for an Attribute whose carriers cluster away from
/// the selection's start, where the walk is slowest. There 20 000 carriers
/// holding the last of 1 000 000 identifiers cost 41 ms read against 770 ms
/// walked.
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
                && selection.q.is_none()
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
    let rows = EntityRowRefs::unaliased(schema.cloned()).standing_at(selection.standing);
    let carried = attrs_carriers(selection, names, &rows)?;
    let mut carriers = Query::select();
    carriers
        .expr(rows.search_column(EntitySearch::EntityKey))
        .from(rows.table(EntitySearch::Table))
        .cond_where(carried);
    Some(carriers.take())
}

/// The projected page of `source`, a selection by `attrs` alone, cut from its
/// `carriers`, and the order its rows come back in, the carriers branch's
/// rows carrying the total when `carried` asks for it.
///
/// # Errors
///
/// Fails as rendering the plain page fails.
pub(crate) fn carrier_page(source: &PageSource<'_>, carriers: SelectStatement, carried: CountPolicy) -> Result<(ProjectedPage, ReadOrder), RenderError> {
    let identity = EntityRelations {
        source: RowSource::Entities,
        search: Demand::NotJoined,
    };
    let probed = EntityRowRefs::aliased(source.schema.cloned(), Walked, Probed).standing_at(source.rows.standing());
    let mut probe = Query::select();
    probe.expr(Expr::val(1)).from_as(probed.table(EntitySearch::Table), Probed).cond_where(
        build_entity_conditions(source.query, source.names, &probed, &RowSource::Entities, QFilter::Whole, GeoFilter::Matched)?
            .add(Expr::col((Probed, EntitySearch::EntityKey)).eq(probed.entity_key_column())),
    );
    let walk = PageWalk {
        source: PageSource {
            relations: &identity,
            ..*source
        },
        walked: Condition::all(),
        probe: Condition::all().add(Expr::exists(probe.take())),
        short: CARRIER_SHORT_WALK,
    };
    let read = IndexRead {
        candidates: carriers,
        bound: CandidateBound::AtMost(CARRIER_BOUND),
        narrowed: source.condition(QFilter::Whole, GeoFilter::Matched)?,
    };
    // The total is the plain count over the carriers whichever step answers,
    // so the rows name none.
    let columns = GatedColumns {
        total: carried,
        step: StepColumn::Unnamed,
    };
    gated_page(source, walk, vec![read], columns)
}

#[cfg(test)]
mod tests {
    use crate::{entity::build_query, entity_total::EntityTotal};
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_query::{
        count_policy::CountPolicy,
        entity::compiled::CompiledEntityQuery,
        entity_selection::{EntitySelection, EntitySelector},
        filter::{compiled_filter::CompiledFilter, language_matching::LanguageMatching, parser::parse},
        ordering::term::{CompiledOrderTerm, OrderDirection, OrderTarget},
        page_window::PageWindow,
        type_selection::grammar::TypeExpr,
    };
    use andromeda_postgresql_query::query_names::QueryNames;
    use andromeda_postgresql_seaquery::{assert_statement_snapshot, statement_planning::StatementPlanning};
    use andromeda_postgresql_stored_values::{attribute_name_key::AttributeNameKey, entity_type_name_key::EntityTypeNameKey, resolved_names::ResolvedNames};
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;
    use sea_query::SelectStatement;

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
            ..QueryNames::default()
        }
    }

    fn by_attrs(attrs: &[&str]) -> CompiledEntityQuery {
        let mut query = CompiledEntityQuery::builder().selection(EntitySelection::builder().build()).build();
        query.selection.selectors = vec![EntitySelector::builder().build()];
        query.selection.attrs = attrs.iter().map(|attr| iri(&format!("https://example.com/{attr}"))).collect();
        query
    }

    fn page_select(query: &CompiledEntityQuery, window: &PageWindow) -> SelectStatement {
        build_query(&ContextTarget::Live, query, window, CountPolicy::Omit, &names())
            .expect("valid entity query")
            .select
    }

    #[test]
    fn an_attrs_only_selection_walks_short_then_reads_the_attribute_index() {
        assert_statement_snapshot!(
            "carrier-page-an-attrs-only-selection-walks-short-then-reads-the-attribute-index",
            page_select(&by_attrs(&["headSign"]), &PageWindow::offset(100, 0))
        );
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

    /// Clause 6.3.13: the carriers branch reads every carrier, so it carries
    /// the total; the short walk sees part of the selection and the full walk
    /// stops at the page, so when either answers the total is its own count
    /// over every carrier.
    #[test]
    fn a_counted_carrier_page_carries_its_total_on_the_carriers_branch_alone() {
        let built = build_query(
            &ContextTarget::Live,
            &by_attrs(&["headSign"]),
            &PageWindow::offset(100, 0),
            CountPolicy::Emit,
            &names(),
        )
        .expect("valid entity query");
        let EntityTotal::OnPageRows { uncounted } = built.total else {
            panic!("a first carrier page carries its total when the carriers answer");
        };
        assert_statement_snapshot!(
            "carrier-page-a-counted-carrier-page-carries-its-total-on-the-carriers-branch-alone-1",
            uncounted.count
        );
        assert_eq!(built.total_planning, StatementPlanning::Cached);
        assert_statement_snapshot!(
            "carrier-page-a-counted-carrier-page-carries-its-total-on-the-carriers-branch-alone-2",
            built.select
        );
    }

    /// Clause 5.7.2.4: an Entity carrying any one of the Attributes matches,
    /// and a name no Entity has carried selects nothing.
    #[test]
    fn the_carriers_of_several_attributes_are_read_as_one_disjunction() {
        assert_statement_snapshot!(
            "carrier-page-the-carriers-of-several-attributes-are-read-as-one-disjunction",
            page_select(&by_attrs(&["headSign", "temperature", "neverWritten"]), &PageWindow::offset(100, 0))
        );
    }

    #[test]
    fn a_cursor_page_of_carriers_walks_from_its_cursor() {
        let boundary = "urn:ngsi-ld:Trip:042".parse().expect("valid identifier");
        assert_statement_snapshot!(
            "carrier-page-a-cursor-page-of-carriers-walks-from-its-cursor",
            page_select(&by_attrs(&["headSign"]), &PageWindow::after(20, boundary))
        );
    }

    #[test]
    fn a_page_deeper_than_the_short_walk_visits_no_entity_before_the_carriers() {
        assert_statement_snapshot!(
            "carrier-page-a-page-deeper-than-the-short-walk-visits-no-entity-before-the-carriers",
            page_select(&by_attrs(&["headSign"]), &PageWindow::offset(100, 30_000))
        );
    }

    #[test]
    fn an_attrs_selection_beside_a_type_keeps_the_type_walk() {
        let mut query = by_attrs(&["headSign"]);
        query.selection.selectors = vec![
            EntitySelector::builder()
                .type_expr(Some(TypeExpr::Type(iri("https://example.com/Trip"))))
                .build(),
        ];
        assert_statement_snapshot!(
            "carrier-page-an-attrs-selection-beside-a-type-keeps-the-type-walk",
            page_select(&query, &PageWindow::offset(100, 0))
        );
    }

    #[test]
    fn an_attrs_selection_filtered_further_keeps_the_plain_page() {
        let mut filtered = by_attrs(&["headSign"]);
        filtered.selection.q = Some(CompiledFilter::new(
            parse("\"https://example.com/temperature\"!=1").expect("valid q"),
            LanguageMatching::TagEquality,
        ));
        let mut ordered = by_attrs(&["headSign"]);
        ordered.ordering.terms = vec![CompiledOrderTerm {
            target: OrderTarget::ModifiedAt,
            direction: OrderDirection::Desc,
            collation: None,
        }];
        for (index, query) in [filtered, ordered, by_attrs(&["neverWritten"])].into_iter().enumerate() {
            let built = build_query(&ContextTarget::Live, &query, &PageWindow::offset(100, 0), CountPolicy::Omit, &names()).expect("valid entity query");
            assert_statement_snapshot!(
                format!("carrier-page-an-attrs-selection-filtered-further-keeps-the-plain-page-{}", index + 1),
                built.select
            );
            assert_eq!(built.planning, StatementPlanning::Cached);
        }
    }
}
