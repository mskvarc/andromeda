//! A geometry of a million positions decodes into its positions without an
//! intermediate tree (clause 4.7): the decode's peak heap stays below 64 MiB
//! above the input, where 1,000,000 positions of 32 bytes are about 32 MB.
//!
//! The counting allocator is this test binary's global allocator, so it
//! measures nothing but this test.

use andromeda_ngsi_ld_entity::{entity_state::EntityState, geo_value::GeoValue};
use andromeda_ngsi_ld_geometry::form::GeometryForm;
use andromeda_test_allocator::counting_allocator::CountingAllocator;

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator::new();

/// The positions of the test polygon's one ring, closing position included.
const POSITIONS: usize = 1_000_000;

/// The decode may peak this far above what was allocated before it.
const PEAK_BUDGET: usize = 64 * 1024 * 1024;

/// The stored document of an entity whose `location` is a Polygon of one ring
/// of [`POSITIONS`] positions.
fn polygon_document() -> String {
    let mut text = String::with_capacity(POSITIONS * 64 + 1024);
    text.push_str(r#"[{"@id":"urn:ngsi-ld:Area:1","@type":["https://ex.org/Area"],"https://uri.etsi.org/ngsi-ld/location":[{"@type":["https://uri.etsi.org/ngsi-ld/GeoProperty"],"https://uri.etsi.org/ngsi-ld/hasValue":[{"@type":["https://purl.org/geojson/vocab#Polygon"],"https://purl.org/geojson/vocab#coordinates":[{"@list":[{"@list":["#);
    for index in 0..POSITIONS {
        let step = if index + 1 == POSITIONS { 0 } else { index };
        let angle = f64::from(u32::try_from(step).unwrap_or(0)) * 0.000_006;
        let separator = if index == 0 { "" } else { "," };
        text.push_str(separator);
        text.push_str(r#"{"@list":[{"@value":"#);
        text.push_str(&(13.0 + angle.cos()).to_string());
        text.push_str(r#"},{"@value":"#);
        text.push_str(&(52.0 + angle.sin()).to_string());
        text.push_str("}]}");
    }
    text.push_str("]}]}]}]}]}]");
    text
}

#[test]
fn a_geometry_node_decodes_into_its_positions_without_a_tree() {
    let document = polygon_document();
    let mark = ALLOCATOR.mark();

    let state = EntityState::from_slice(document.as_bytes()).expect("the polygon decodes");

    let peak_above_input = ALLOCATOR.peak_above(mark);
    let positions = state
        .attributes()
        .get("https://uri.etsi.org/ngsi-ld/location")
        .and_then(|instances| instances.get(None))
        .and_then(|instance| instance.value().geometry())
        .map(GeoValue::form)
        .map(|form| match form {
            GeometryForm::Polygon(rings) => rings.rings().iter().map(|ring| ring.positions().len()).sum::<usize>(),
            GeometryForm::Point(_)
            | GeometryForm::MultiPoint(_)
            | GeometryForm::LineString(_)
            | GeometryForm::MultiLineString(_)
            | GeometryForm::MultiPolygon(_) => 0,
        });
    assert_eq!(positions, Some(POSITIONS));
    assert!(
        peak_above_input < PEAK_BUDGET,
        "decoding {POSITIONS} positions peaked at {peak_above_input} bytes above the input, over the {PEAK_BUDGET} byte budget"
    );
}
