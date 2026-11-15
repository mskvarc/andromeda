//! A trailing path reads the `bbox` and the foreign members of a geometry
//! alike in a decoded state and in a written Temporal Evolution document
//! (clause 4.9, IETF RFC 7946 sections 5 and 6.1).

use crate::{document::EntityDocument, entity_state::EntityState};
use andromeda_ngsi_ld_query::filter::{language_matching::LanguageMatching, parser::parse, reading::filter_reading::FilterReading};
use andromeda_ngsi_ld_query_evaluation::{
    q::filter_run::filter_holds,
    queried_entity::{attribute_reading::QueriedEntity, linked_entities::NoLinkedEntities},
};
use serde_json::{Value, json};

/// A Kiosk whose `site` is the Point 13.4, 52.5 with the `bbox` 13.3, 52.4,
/// 13.5, 52.6, the foreign member `title` "Gate" and the foreign member `meta`
/// holding `source` "survey".
fn kiosk() -> Value {
    json!([{
        "@id": "urn:ngsi-ld:Kiosk:1",
        "@type": ["https://example.com/Kiosk"],
        "https://example.com/site": [{
            "@type": ["https://uri.etsi.org/ngsi-ld/GeoProperty"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{
                "@type": ["https://purl.org/geojson/vocab#Point"],
                "https://purl.org/geojson/vocab#coordinates": [{"@list": [{"@value": 13.4}, {"@value": 52.5}]}],
                "https://purl.org/geojson/vocab#bbox": [{"@list": [{"@value": 13.3}, {"@value": 52.4}, {"@value": 13.5}, {"@value": 52.6}]}],
                "https://example.com/title": [{"@value": "Gate"}],
                "https://example.com/meta": [{"https://example.com/source": [{"@value": "survey"}]}]
            }]
        }]
    }])
}

/// Every term over the Kiosk with the answer clause 4.9 gives: the `bbox` is
/// an array the equality operators read by membership and no ordering
/// matches, a foreign member and the members within it are read by expanded
/// name, a structured value is unequal to a String, and the expanded name of
/// the `bbox` names no foreign member.
const TERMS: [(&str, bool); 13] = [
    ("\"https://example.com/site\"[bbox]", true),
    ("\"https://example.com/site\"[bbox]==13.3", true),
    ("\"https://example.com/site\"[bbox]==13.35", false),
    ("\"https://example.com/site\"[bbox]>0", false),
    ("\"https://example.com/site\"[bbox]!=13.3", false),
    ("\"https://example.com/site\"[\"https://example.com/title\"]==\"Gate\"", true),
    ("\"https://example.com/site\"[\"https://example.com/title\"]==\"Door\"", false),
    (
        "\"https://example.com/site\"[\"https://example.com/meta\".\"https://example.com/source\"]==\"survey\"",
        true,
    ),
    ("\"https://example.com/site\"[\"https://example.com/meta\"]!=\"x\"", true),
    ("\"https://example.com/site\"[\"https://example.com/missing\"]", false),
    ("\"https://example.com/site\"[\"https://purl.org/geojson/vocab#bbox\"]", false),
    ("\"https://example.com/site\"[type]==\"Point\"", true),
    ("\"https://example.com/site\"[coordinates]==13.4", true),
];

/// Whether each term holds over `entity`, beside the answer expected.
fn answers<'a>(entity: impl QueriedEntity<'a>) -> Vec<(&'static str, bool, bool)> {
    TERMS
        .iter()
        .map(|(q, expected)| {
            let expr = parse(q).expect("the Query Term parses");
            let held = filter_holds(&FilterReading::of(&expr, LanguageMatching::TagEquality), entity, NoLinkedEntities);
            (*q, held, *expected)
        })
        .collect()
}

#[test]
fn a_state_reads_the_bbox_and_the_foreign_members_of_a_geometry() {
    let state = EntityState::from_slice(&serde_json::to_vec(&kiosk()).expect("JSON")).expect("a stored entity decodes");
    for (q, held, expected) in answers(&state) {
        pretty_assertions::assert_eq!(held, expected, "{q}");
    }
}

#[test]
fn a_written_document_reads_the_bbox_and_the_foreign_members_of_a_geometry() {
    let document = EntityDocument::from_slice(&serde_json::to_vec(&kiosk()).expect("JSON")).expect("a written document decodes");
    for (q, held, expected) in answers(&document) {
        pretty_assertions::assert_eq!(held, expected, "{q}");
    }
}
