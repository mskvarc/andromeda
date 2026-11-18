//! Every page shape judges clause 4.22 expiry at the instant its selection
//! fixed: no statement it runs reads the database clock, and the instant is
//! bound into it.

use crate::{
    entity::{EntityQuery, build_ids_query, build_query},
    entity_total::{EntityTotal, TotalCount},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry, positive_distance::PositiveDistance};
use andromeda_ngsi_ld_query::{
    count_policy::CountPolicy,
    entity::compiled::CompiledEntityQuery,
    entity_selection::EntitySelection,
    expanded_entity_selector::ExpandedEntitySelector,
    filter::{Filter, compiled_filter::CompiledFilter, language_matching::LanguageMatching, value_path::ValuePath},
    geo::compiled_geo_query::CompiledGeoQuery,
    ordering::{
        compiled_ordering_term::CompiledOrderingTerm,
        expanded_ordering_target::ExpandedOrderingTarget,
        ordering_direction::OrderingDirection,
        ordering_geometry::OrderingGeometry,
    },
    page_window::{PageStart, PageWindow},
    pattern::Pattern,
    scope::scope_query::ScopeQuery,
    standing_instant::StandingInstant,
    type_selection::expanded_type_selection::ExpandedTypeSelection,
};
use andromeda_ngsi_ld_vocabulary::expanded::properties::LOCATION;
use andromeda_postgresql_query::{attribute_carriers::AttributeCarriers, query_names::QueryNames};
use andromeda_postgresql_stored_values::{attribute_name_key::AttributeNameKey, entity_type_name_key::EntityTypeNameKey, resolved_names::ResolvedNames};
use chrono::{DateTime, Utc};
use iri_rs::IriBuf;
use pretty_assertions::assert_eq;
use sea_query::{PostgresQueryBuilder, SelectStatement, Value};
use std::str::FromStr;

const TEMPERATURE: &str = "https://example.com/temperature";

const HEAD_SIGN: &str = "https://example.com/headSign";

/// The dictionary keys, and a census counting 50 instances of `headSign`.
fn names() -> QueryNames {
    let head_sign = AttributeNameKey::from(28);
    QueryNames {
        carriers: AttributeCarriers::new(&[head_sign], [(head_sign, 50)]),
        attributes: ResolvedNames::from_pairs(
            [
                ("temperature", 12),
                ("source", 13),
                ("hasTrip", 14),
                ("headSign", 28),
                ("speed", 7),
                ("accuracy", 8),
            ]
            .map(|(name, key)| (format!("https://example.com/{name}").parse().expect("valid name"), AttributeNameKey::from(key)))
            .into_iter()
            .chain([(LOCATION.as_str().parse().expect("valid name"), AttributeNameKey::from(9))]),
        ),
        types: ResolvedNames::from_pairs([("Vehicle", 1), ("Trip", 3), ("StopTime", 18)].map(|(name, key)| {
            (
                format!("https://example.com/{name}").parse().expect("valid type name"),
                EntityTypeNameKey::from(key),
            )
        })),
        ..QueryNames::default()
    }
}

fn of_type(name: &str) -> ExpandedEntitySelector {
    ExpandedEntitySelector::builder()
        .type_selection(Some(ExpandedTypeSelection::Type(
            IriBuf::new(format!("https://example.com/{name}")).expect("valid IRI"),
        )))
        .build()
}

fn query(selection: EntitySelection) -> CompiledEntityQuery {
    CompiledEntityQuery::builder().selection(selection).build()
}

fn filtered(q: &str) -> CompiledEntityQuery {
    query(
        EntitySelection::builder()
            .q(Some(CompiledFilter::new(q.parse::<Filter>().expect("valid q"), LanguageMatching::TagEquality)))
            .build(),
    )
}

fn ordered_by(direction: OrderingDirection, path: ValuePath) -> CompiledEntityQuery {
    let mut ordered = query(EntitySelection::builder().selectors(vec![of_type("Vehicle")]).build());
    ordered.ordering.terms = vec![CompiledOrderingTerm {
        target: ExpandedOrderingTarget::Attribute(path),
        direction,
        collation: None,
    }];
    ordered
}

fn attribute(name: &str) -> ValuePath {
    ValuePath::simple(format!("https://example.com/{name}").parse().expect("valid attribute name"))
}

fn point() -> Geometry {
    serde_json::from_value::<Geometry>(serde_json::json!({ "type": "Point", "coordinates": [13.4, 52.5] })).expect("point parses")
}

/// The Vehicles within 1 000 m of the point.
fn vehicles_near() -> CompiledEntityQuery {
    let near = CompiledGeoQuery {
        georel: GeoRel::Near {
            max_distance: Some(PositiveDistance::from_str("1000").expect("positive distance")),
            min_distance: None,
        },
        geometry: point(),
        geoproperty: LOCATION.as_str().parse().expect("valid attribute name"),
    };
    query(EntitySelection::builder().selectors(vec![of_type("Vehicle")]).geo(Some(near)).build())
}

/// The page shapes of a geo-query, named, with the window and count each is
/// read under: the plain page of a counted one, the geo page of an uncounted
/// one, and the keyed page of a keyed `q` beside one.
fn geo_page_shapes() -> Vec<(&'static str, CompiledEntityQuery, PageWindow, CountPolicy)> {
    let mut keyed = vehicles_near();
    keyed.selection.q = Some(CompiledFilter::new(
        format!("\"{TEMPERATURE}\"~=\"text-71.*\"").parse::<Filter>().expect("valid q"),
        LanguageMatching::TagEquality,
    ));
    vec![
        ("geo query", vehicles_near(), PageWindow::offset(100, 0), CountPolicy::Emit),
        ("geo page", vehicles_near(), PageWindow::offset(100, 0), CountPolicy::Omit),
        ("keyed beside geo", keyed, PageWindow::offset(100, 0), CountPolicy::Emit),
    ]
}

/// The gated pages that read the filters their walks test, named, with the
/// window and count each is read under: a keyed page beside a `scopeQ`, and a
/// geo page beside an `attrs` the census bounds.
fn filter_read_shapes() -> Vec<(&'static str, CompiledEntityQuery, PageWindow, CountPolicy)> {
    let mut scoped = filtered(&format!("\"{TEMPERATURE}\"~=\"text-71.*\""));
    scoped.selection.scope_q = Some("/Madrid/#".parse::<ScopeQuery>().expect("valid scope query"));
    let mut carried = vehicles_near();
    carried.selection.attrs = vec![IriBuf::new(HEAD_SIGN.to_owned()).expect("valid IRI")];
    vec![
        ("keyed beside scope", scoped, PageWindow::offset(100, 0), CountPolicy::Emit),
        ("geo page beside counted attrs", carried, PageWindow::offset(100, 0), CountPolicy::Omit),
    ]
}

/// One query of each page shape, named, with the window and count it is read
/// under.
fn every_page_shape() -> Vec<(&'static str, CompiledEntityQuery, PageWindow, CountPolicy)> {
    let first_page = || PageWindow::offset(100, 0);
    let mut by_distance = ordered_by(
        OrderingDirection::DistAsc,
        ValuePath::simple(LOCATION.as_str().parse().expect("valid attribute name")),
    );
    by_distance.ordering.geometry = Some(OrderingGeometry::new(point()));
    let mut by_sub_attribute = attribute("speed");
    by_sub_attribute.sub_attributes = vec!["https://example.com/accuracy".parse().expect("valid attribute name")];
    let mut shapes = vec![
        ("plain", query(EntitySelection::builder().build()), first_page(), CountPolicy::Emit),
        (
            "type walk",
            query(EntitySelection::builder().selectors(vec![of_type("Vehicle")]).build()),
            PageWindow {
                limit: 500,
                start: PageStart::Cursor(None),
            },
            CountPolicy::Emit,
        ),
        ("keyed", filtered(&format!("\"{TEMPERATURE}\"~=\"text-71.*\"")), first_page(), CountPolicy::Emit),
        (
            "keyed sub-attribute",
            filtered(&format!("\"{TEMPERATURE}\".\"https://example.com/source\"==\"sensor-43\"")),
            first_page(),
            CountPolicy::Emit,
        ),
        (
            "keyed hop",
            filtered("\"https://example.com/hasTrip\"{\"https://example.com/headSign\"}==\"Airport\""),
            first_page(),
            CountPolicy::Emit,
        ),
        ("keyed existence", filtered(&format!("\"{TEMPERATURE}\"")), first_page(), CountPolicy::Emit),
        (
            "carrier",
            query(
                EntitySelection::builder()
                    .selectors(vec![ExpandedEntitySelector::builder().build()])
                    .attrs(vec![IriBuf::new(HEAD_SIGN.to_owned()).expect("valid IRI")])
                    .build(),
            ),
            first_page(),
            CountPolicy::Emit,
        ),
        (
            "value walk",
            ordered_by(OrderingDirection::Asc, attribute("speed")),
            first_page(),
            CountPolicy::Emit,
        ),
        (
            "sub-attribute ordering",
            ordered_by(OrderingDirection::Asc, by_sub_attribute),
            first_page(),
            CountPolicy::Emit,
        ),
        ("distance ordering", by_distance, first_page(), CountPolicy::Emit),
        (
            "id pattern",
            query(
                EntitySelection::builder()
                    .selectors(vec![
                        ExpandedEntitySelector::builder()
                            .type_selection(of_type("StopTime").type_selection)
                            .id_pattern(Some(".*_1196287$".parse::<Pattern>().expect("valid pattern")))
                            .build(),
                    ])
                    .build(),
            ),
            first_page(),
            CountPolicy::Emit,
        ),
    ];
    shapes.extend(geo_page_shapes());
    shapes.extend(filter_read_shapes());
    shapes
}

/// Every statement a query runs: its page, every count it may run, and its
/// identifiers alone.
fn statements(compiled: &CompiledEntityQuery, window: &PageWindow, count: CountPolicy) -> Vec<SelectStatement> {
    let EntityQuery { select, total, .. } = build_query(&ContextTarget::Live, compiled, window, count, &names()).expect("valid entity query");
    let ids = build_ids_query(&ContextTarget::Live, compiled, window, &names())
        .expect("valid identifier query")
        .select;
    let counts = match total {
        EntityTotal::Omitted => Vec::new(),
        EntityTotal::OnPageRows { uncounted: total } | EntityTotal::Counted(total) => {
            let TotalCount { count, walked } = total;
            [count].into_iter().chain(walked.map(|walked| walked.select)).collect()
        }
    };
    [select, ids].into_iter().chain(counts).collect()
}

fn at(standing: StandingInstant, mut compiled: CompiledEntityQuery) -> CompiledEntityQuery {
    compiled.selection.standing = standing;
    compiled
}

/// Clause 4.22 decides expiry at one instant: a read at its own instant asks
/// the transaction clock in every statement, and a read at a fixed instant
/// asks it in none and binds the instant in each.
#[test]
fn every_page_shape_judges_expiry_at_the_instant_its_selection_fixed() {
    let fixed = DateTime::<Utc>::from_timestamp(1_893_456_000, 0).expect("a valid instant");
    for (shape, compiled, window, count) in every_page_shape() {
        let read_now = statements(&at(StandingInstant::WhenRead, compiled.clone()), &window, count);
        let read_at = statements(&at(StandingInstant::At(fixed), compiled), &window, count);
        for (index, (now, fixed_statement)) in read_now.iter().zip(&read_at).enumerate() {
            let now_sql = now.to_string(PostgresQueryBuilder);
            let (fixed_sql, values) = fixed_statement.build(PostgresQueryBuilder);
            assert_eq!(
                (
                    shape,
                    index,
                    now_sql.contains("NOW()"),
                    fixed_sql.contains("NOW()"),
                    values.0.contains(&Value::from(fixed))
                ),
                (shape, index, true, false, true),
                "{fixed_sql}"
            );
        }
    }
}
