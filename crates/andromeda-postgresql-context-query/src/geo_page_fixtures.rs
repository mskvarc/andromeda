//! The queries the page shape, geo page, keyed page, filter read and keyed
//! count tests share: the Entities of type `Sensor`, and those within the
//! square from `[0, 40]` to `[10, 50]`, read against a dictionary that has
//! recorded `location`.

use crate::entity::{EntityQuery, build_query};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry};
use andromeda_ngsi_ld_query::{
    count_policy::CountPolicy,
    entity::compiled::CompiledEntityQuery,
    entity_selection::EntitySelection,
    expanded_entity_selector::ExpandedEntitySelector,
    filter::{compiled_filter::CompiledFilter, language_matching::LanguageMatching, parser::parse},
    geo::compiled_geo_query::CompiledGeoQuery,
    page_window::PageWindow,
    type_selection::expanded_type_selection::ExpandedTypeSelection,
};
use andromeda_ngsi_ld_vocabulary::expanded::properties::LOCATION;
use andromeda_postgresql_query::query_names::QueryNames;
use andromeda_postgresql_stored_values::{attribute_name_key::AttributeNameKey, entity_type_name_key::EntityTypeNameKey, resolved_names::ResolvedNames};
use iri_rs::IriBuf;

/// The expanded `observationSpace`, a `GeoProperty` the core context names.
pub(crate) const OBSERVATION_SPACE: &str = "https://uri.etsi.org/ngsi-ld/observationSpace";

/// `temperature` at key 12, `hasTrip` at 14, `status` at 15, `calibration`
/// at 16, `inspection` at 17, `location` at 9 and `observationSpace` at 42;
/// the type `Sensor` at key 3.
pub(crate) fn names() -> QueryNames {
    QueryNames {
        attributes: ResolvedNames::from_pairs(
            [("temperature", 12), ("hasTrip", 14), ("status", 15), ("calibration", 16), ("inspection", 17)]
                .map(|(name, key)| (format!("https://example.com/{name}").parse().expect("valid name"), AttributeNameKey::from(key)))
                .into_iter()
                .chain([
                    (LOCATION.as_str().parse().expect("valid name"), AttributeNameKey::from(9)),
                    (OBSERVATION_SPACE.parse().expect("valid name"), AttributeNameKey::from(42)),
                ]),
        ),
        types: ResolvedNames::from_pairs([("https://example.com/Sensor".parse().expect("valid type name"), EntityTypeNameKey::from(3))]),
        ..QueryNames::default()
    }
}

/// An IRI of the example vocabulary.
pub(crate) fn iri(name: &str) -> IriBuf {
    IriBuf::new(format!("https://example.com/{name}")).expect("valid IRI")
}

/// The Entities of type `Sensor`.
pub(crate) fn sensors() -> CompiledEntityQuery {
    let selection = EntitySelection::builder()
        .selectors(vec![
            ExpandedEntitySelector::builder()
                .type_selection(Some(ExpandedTypeSelection::Type(iri("Sensor"))))
                .build(),
        ])
        .build();
    CompiledEntityQuery::builder().selection(selection).build()
}

/// The Entities of type `Sensor` whose `location` lies within the square.
pub(crate) fn sensors_within_square() -> CompiledEntityQuery {
    let square = serde_json::from_value::<Geometry>(serde_json::json!({
        "type": "Polygon",
        "coordinates": [[[0.0, 40.0], [10.0, 40.0], [10.0, 50.0], [0.0, 50.0], [0.0, 40.0]]],
    }))
    .expect("square parses");
    let mut query = sensors();
    query.selection.geo = Some(CompiledGeoQuery {
        georel: GeoRel::Within,
        geometry: square,
        geoproperty: LOCATION.as_str().parse().expect("valid attribute name"),
    });
    query
}

/// `query` filtered by the `q` expression `q`.
pub(crate) fn filtered(mut query: CompiledEntityQuery, q: &str) -> CompiledEntityQuery {
    query.selection.q = Some(CompiledFilter::new(parse(q).expect("valid q"), LanguageMatching::TagEquality));
    query
}

/// The first offset page of 100.
pub(crate) fn first_page() -> PageWindow {
    PageWindow::offset(100, 0)
}

/// The page of 100 past `urn:ngsi-ld:Sensor:s1`.
pub(crate) fn past_first_sensor() -> PageWindow {
    PageWindow::after(100, "urn:ngsi-ld:Sensor:s1".parse().expect("valid URI"))
}

/// `query` built over `window` on the live branch, with the total `count`
/// asks for.
pub(crate) fn built(query: &CompiledEntityQuery, window: &PageWindow, count: CountPolicy) -> EntityQuery {
    build_query(&ContextTarget::Live, query, window, count, &names()).expect("valid entity query")
}
