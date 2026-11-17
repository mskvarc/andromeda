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
    entity_selection::{EntitySelection, EntitySelector},
    filter::{compiled_filter::CompiledFilter, language_matching::LanguageMatching, parser::parse, path::AttrPath},
    geo::query::GeoQuery,
    ordering::{
        geometry::OrderingGeometry,
        term::{CompiledOrderTerm, OrderDirection, OrderTarget},
    },
    page_window::{PageStart, PageWindow},
    pattern::Pattern,
    standing_instant::StandingInstant,
    type_selection::grammar::TypeExpr,
};
use andromeda_ngsi_ld_vocabulary::expanded::properties::LOCATION;
use andromeda_postgresql_query::query_names::QueryNames;
use andromeda_postgresql_stored_values::{attribute_name_key::AttributeNameKey, entity_type_name_key::EntityTypeNameKey, resolved_names::ResolvedNames};
use chrono::{DateTime, Utc};
use iri_rs::IriBuf;
use pretty_assertions::assert_eq;
use sea_query::{PostgresQueryBuilder, SelectStatement, Value};
use std::str::FromStr;

const TEMPERATURE: &str = "https://example.com/temperature";

fn names() -> QueryNames {
    QueryNames {
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

fn of_type(name: &str) -> EntitySelector {
    EntitySelector::builder()
        .type_expr(Some(TypeExpr::Type(IriBuf::new(format!("https://example.com/{name}")).expect("valid IRI"))))
        .build()
}

fn query(selection: EntitySelection) -> CompiledEntityQuery {
    CompiledEntityQuery::builder().selection(selection).build()
}

fn filtered(q: &str) -> CompiledEntityQuery {
    query(
        EntitySelection::builder()
            .q(Some(CompiledFilter::new(parse(q).expect("valid q"), LanguageMatching::TagEquality)))
            .build(),
    )
}

fn ordered_by(direction: OrderDirection, path: AttrPath) -> CompiledEntityQuery {
    let mut ordered = query(EntitySelection::builder().selectors(vec![of_type("Vehicle")]).build());
    ordered.ordering.terms = vec![CompiledOrderTerm {
        target: OrderTarget::Attribute(path),
        direction,
        collation: None,
    }];
    ordered
}

fn attribute(name: &str) -> AttrPath {
    AttrPath::simple(format!("https://example.com/{name}").parse().expect("valid attribute name"))
}

fn point() -> Geometry {
    serde_json::from_value::<Geometry>(serde_json::json!({ "type": "Point", "coordinates": [13.4, 52.5] })).expect("point parses")
}

/// One query of each page shape, named, with the window and count it is read
/// under.
fn every_page_shape() -> Vec<(&'static str, CompiledEntityQuery, PageWindow, CountPolicy)> {
    let first_page = || PageWindow::offset(100, 0);
    let near = GeoQuery {
        georel: GeoRel::Near {
            max_distance: Some(PositiveDistance::from_str("1000").expect("positive distance")),
            min_distance: None,
        },
        geometry: point(),
        geoproperty: LOCATION.as_str().parse().expect("valid attribute name"),
    };
    let mut by_distance = ordered_by(
        OrderDirection::DistAsc,
        AttrPath::simple(LOCATION.as_str().parse().expect("valid attribute name")),
    );
    by_distance.ordering.geometry = Some(OrderingGeometry::new(point()));
    let mut by_sub_attribute = attribute("speed");
    by_sub_attribute.sub_attrs = vec!["https://example.com/accuracy".parse().expect("valid attribute name")];
    vec![
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
        (
            "geo query",
            query(EntitySelection::builder().selectors(vec![of_type("Vehicle")]).geo(Some(near.clone())).build()),
            first_page(),
            CountPolicy::Emit,
        ),
        (
            "geo page",
            query(EntitySelection::builder().selectors(vec![of_type("Vehicle")]).geo(Some(near)).build()),
            first_page(),
            CountPolicy::Omit,
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
                    .selectors(vec![EntitySelector::builder().build()])
                    .attrs(vec![IriBuf::new("https://example.com/headSign".to_owned()).expect("valid IRI")])
                    .build(),
            ),
            first_page(),
            CountPolicy::Emit,
        ),
        (
            "value walk",
            ordered_by(OrderDirection::Asc, attribute("speed")),
            first_page(),
            CountPolicy::Emit,
        ),
        (
            "sub-attribute ordering",
            ordered_by(OrderDirection::Asc, by_sub_attribute),
            first_page(),
            CountPolicy::Emit,
        ),
        ("distance ordering", by_distance, first_page(), CountPolicy::Emit),
        (
            "id pattern",
            query(
                EntitySelection::builder()
                    .selectors(vec![
                        EntitySelector::builder()
                            .type_expr(of_type("StopTime").type_expr)
                            .id_pattern(Some(Pattern::parse(".*_1196287$").expect("valid pattern")))
                            .build(),
                    ])
                    .build(),
            ),
            first_page(),
            CountPolicy::Emit,
        ),
    ]
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
