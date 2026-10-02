//! The `bbox` and the foreign members of a geometry (IETF RFC 7946 sections 5
//! and 6.1), expanded by the typed walk and by the processor alike, kept for a
//! `GeoProperty` and for a Property whose value is a geometry (clauses 4.6.4,
//! 4.7).

use crate::{
    expansion_fixture::{BothWays, decoded_both_ways, engine, entity},
    normalization::Normalization,
};
use andromeda_ngsi_ld_entity::{
    document::{
        EntityDocument,
        instance_document::InstanceDocument,
        written_value::{ValueSlot, WrittenValue},
    },
    expanded_value::ExpandedValue,
    geo_value::GeoValue,
    literal::LiteralValue,
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use serde_json::{Value, json};

/// The core context's default vocabulary (clause 5.5.7).
const DEFAULT: &str = "https://uri.etsi.org/ngsi-ld/default-context/";

/// The one instance written for `attribute`.
fn instance<'a>(document: &'a EntityDocument, attribute: &str) -> &'a InstanceDocument {
    let member = document.attributes().get(attribute).unwrap_or_else(|| panic!("{attribute} is written"));
    let [instance] = member.instances() else {
        panic!("{attribute} carries one instance");
    };
    instance
}

/// A Point at `(1, 2)` with a box and a `plotName` foreign member.
fn point_with_members() -> Value {
    json!({"type": "Point", "coordinates": [1, 2], "bbox": [1, 2, 1, 2], "plotName": "x"})
}

/// The plain string of a one-literal member.
fn text(values: &[ExpandedValue]) -> Option<&str> {
    match values {
        [ExpandedValue::Literal(literal)] => match literal.value() {
            LiteralValue::String(text) => Some(text.as_str()),
            LiteralValue::Number(_) | LiteralValue::Boolean(_) => None,
        },
        [] | [_, ..] => None,
    }
}

/// Clause 4.7.1, Annex B: a `GeoProperty` keeps the box and the foreign member
/// its geometry was written with, the foreign member under `@vocab`.
#[tokio::test]
async fn a_geo_property_box_and_foreign_members_expand_into_the_model() {
    let document = entity(
        &engine(),
        json!({"id": "urn:ngsi-ld:Parcel:1", "type": "Parcel", "area": {"type": "GeoProperty", "value": point_with_members()}}),
        Normalization::Entity,
    )
    .await
    .expect("the fixture expands")
    .agreed();

    let ValueSlot::Set(WrittenValue::Geometry(GeoValue::Geometry(area))) = instance(&document, &format!("{DEFAULT}area")).value() else {
        panic!("a geometry");
    };
    let members = area.members().expect("the members are kept");
    pretty_assertions::assert_eq!(members.bbox().map(|bbox| bbox.ordinates().collect::<Vec<_>>()), Some(vec![1.0, 2.0, 1.0, 2.0]));
    let title = format!("{DEFAULT}plotName");
    pretty_assertions::assert_eq!(members.foreign().get(title.as_str()).and_then(|values| text(values)), Some("x"));
}

/// Clause 5.5.7: a foreign member named by a user term expands to that term's
/// IRI, on both expansion paths.
#[tokio::test]
async fn a_foreign_member_named_by_a_user_term_expands_to_that_term() {
    let document = entity(
        &engine(),
        json!({
            "id": "urn:ngsi-ld:Parcel:2", "type": "Parcel",
            "area": {"type": "GeoProperty", "value": point_with_members()},
            "@context": [{"plotName": "https://ex.org/plotName"}, "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld"]
        }),
        Normalization::Entity,
    )
    .await
    .expect("the fixture expands")
    .agreed();

    let ValueSlot::Set(WrittenValue::Geometry(GeoValue::Geometry(area))) = instance(&document, &format!("{DEFAULT}area")).value() else {
        panic!("a geometry");
    };
    let foreign = area.members().expect("the members are kept").foreign();
    pretty_assertions::assert_eq!(foreign.keys().map(AttributeName::as_str).collect::<Vec<_>>(), vec!["https://ex.org/plotName"]);
}

/// Clause 4.6.4: a Property whose value is a geometry keeps the box and the
/// foreign member in the node it stores, as the processor stores them.
#[tokio::test]
async fn a_property_valued_geometry_keeps_its_box_and_foreign_members() {
    let document = entity(
        &engine(),
        json!({"id": "urn:ngsi-ld:Parcel:3", "type": "Parcel", "note": {"type": "Property", "value": point_with_members()}}),
        Normalization::Entity,
    )
    .await
    .expect("the fixture expands")
    .agreed();

    let ValueSlot::Set(WrittenValue::Value(values)) = instance(&document, &format!("{DEFAULT}note")).value() else {
        panic!("a Property value");
    };
    let [ExpandedValue::Node(node)] = values.as_slice() else {
        panic!("one node");
    };
    let mut names: Vec<&str> = node.members().keys().map(AttributeName::as_str).collect();
    names.sort_unstable();
    let title = format!("{DEFAULT}plotName");
    pretty_assertions::assert_eq!(
        names,
        vec![
            "https://purl.org/geojson/vocab#bbox",
            "https://purl.org/geojson/vocab#coordinates",
            title.as_str()
        ]
    );
}

/// What either expansion writes for a geometry with members, the stored
/// document decoder reads back as the processor decoding reads it.
#[tokio::test]
async fn the_stored_document_decoder_reads_a_geometry_with_members_as_written() {
    for value in [
        json!({"type": "GeoProperty", "value": point_with_members()}),
        json!({"type": "Property", "value": point_with_members()}),
    ] {
        let body = json!({"id": "urn:ngsi-ld:Parcel:4", "type": "Parcel", "area": value});
        let (decoded, canonical) = decoded_both_ways(&engine(), body, Normalization::Entity).await.expect("expands");
        pretty_assertions::assert_eq!(
            decoded.expect("the processor decoding reads it"),
            canonical.expect("the canonical decoder reads it"),
            "{value}"
        );
    }
}

/// RFC 7946 section 5: a box of three numbers is refused on both paths.
#[tokio::test]
async fn a_box_of_three_numbers_is_refused_on_both_paths() {
    let refusal = entity(
        &engine(),
        json!({"id": "urn:ngsi-ld:Parcel:5", "type": "Parcel", "area": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1, 2], "bbox": [1, 2, 3]}}}),
        Normalization::Entity,
    )
    .await
    .map(BothWays::refused);
    match refusal {
        Ok(refusal) => assert!(refusal.contains("RFC 7946 section 5"), "{refusal}"),
        Err(failure) => assert!(failure.reason().contains("RFC 7946 section 5"), "{}", failure.reason()),
    }
}
