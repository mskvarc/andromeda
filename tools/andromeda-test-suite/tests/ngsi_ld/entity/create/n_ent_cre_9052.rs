//! Geojson in a property value is plain json kept as written.

use andromeda_test_suite::{
    attribute_matcher::{attribute_of, property},
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    problem::answered,
    simplified_matcher::has_value_at,
    value::from_json,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{CreateEntity, RepresentationSpelling},
    },
    model::{AttributeKind, Entity, EntityId, Lossless, NgsiLdMediaType},
};

/// The entity as the request writes it.
const BODY: &str = r#"{
    "id": "urn:ngsi-ld:Sketch:{{ns}}",
    "type": "Sketch_{{ns}}",
    "sketch": {
        "type": "Property",
        "value": {
            "type": "Point",
            "coordinates": [
                13.123456789012345678,
                52.5
            ]
        }
    },
    "off": {
        "type": "Property",
        "value": {
            "type": "Point",
            "coordinates": [
                200,
                0,
                1,
                2
            ]
        }
    },
    "group": {
        "type": "Property",
        "value": {
            "type": "GeometryCollection",
            "geometries": [
                {
                    "type": "Point",
                    "coordinates": [
                        13,
                        52
                    ]
                }
            ]
        }
    },
    "track": {
        "type": "ListProperty",
        "valueList": [
            1,
            {
                "type": "Point",
                "coordinates": [
                    1,
                    99
                ]
            }
        ]
    }
}"#;

/// The ordinate with more digits than a double holds, as the answer must spell it.
const KEPT_COORDINATES: &str = "\"coordinates\":[13.123456789012345678,52.5]";

/// Pins: A Sketch whose sketch Property value is the Point [13.123456789012345678, 52.5], whose off Property value
/// is a Point with four ordinates off the axes, whose group Property value is a `GeometryCollection` and whose track
/// `ListProperty` holds such a Point is created, and reads back every value as written: the ordinate keeps every
/// digit, the type stays Property in the normalized and concise representations, and the value comes back
/// unchanged in the simplified one, save the one-element `geometries` array, which compacts to its element
/// Clause: 4.5.2.3, 4.7.1, 4.7.3, 5.2.5, 5.6.1, JSON-LD 1.1 API s6.1
#[gtest]
#[tokio::test]
async fn n_ent_cre_9052_geojson_in_a_property_value_is_plain_json_kept_as_written() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sketch_id = format!("urn:ngsi-ld:Sketch:{namespace}").parse::<EntityId>()?;
    let sketch_type = namespace.entity_type("Sketch")?;
    let sketch = Entity::builder().id(sketch_id.clone()).entity_type(sketch_type).build()?;
    // The model writes an ordinate as the nearest double, so the body is sent as written.
    let request = broker
        .unchecked(&CreateEntity::builder().entity(&sketch).build())?
        .body(Bytes::from(BODY.replace("{{ns}}", namespace.token())));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let received = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&sketch_id)
        .send()
        .await?;
    let entity = received.outcome.body();
    expect_that!(entity, has_attribute("sketch", property()));
    expect_that!(String::from_utf8_lossy(&received.body), contains_substring(KEPT_COORDINATES));
    expect_that!(
        entity,
        has_attribute("off", property().length_at(["coordinates"], 4).value_member(["coordinates", "0"], 200))
    );
    // A Property value is compacted (JSON-LD 1.1 compaction algorithm, compactArrays), so the one-element
    // `geometries` array reads back as its element.
    expect_that!(
        entity,
        has_attribute(
            "group",
            property()
                .value_member(["type"], "GeometryCollection")
                .value_member(["geometries", "coordinates", "1"], 52)
        )
    );
    let track_point = from_json(&serde_json::json!({"type": "Point", "coordinates": [1, 99]})).or_fail()?;
    expect_that!(
        entity,
        has_attribute("track", attribute_of(AttributeKind::ListProperty).list_value(1, track_point))
    );
    let received_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&sketch_id)
        .form(Lossless::Concise)
        .spelling(RepresentationSpelling::default().in_options())
        .send()
        .await?;
    let entity_2 = received_2.outcome.body();
    expect_that!(entity_2, has_attribute("sketch", property()));
    expect_that!(entity_2, has_attribute("off", property()));
    expect_that!(String::from_utf8_lossy(&received_2.body), contains_substring(KEPT_COORDINATES));
    let received_3 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_simplified(&sketch_id)
        .send()
        .await?;
    let entity_3 = received_3.outcome.body();
    expect_that!(entity_3, has_value_at("sketch", ["type"], "Point"));
    expect_that!(String::from_utf8_lossy(&received_3.body), contains_substring(KEPT_COORDINATES));
    expect_that!(entity_3, has_value_at("group", ["type"], "GeometryCollection"));
    Ok(())
}
