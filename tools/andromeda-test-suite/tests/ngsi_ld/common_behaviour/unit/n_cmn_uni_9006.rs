//! A code outside the common codes is refused on every family.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, operation::CreateEntity},
    model::{Entity, ErrorType, Lossless, RawJson},
};

/// Pins: Create Entity with `unitCode` NOTACODE on a Property, a `ListProperty` and a `GeoProperty` answers 400
/// `BadRequestData` each
/// Clause: 4.5.2.2, 4.5.21.2, 4.7.3
#[gtest]
#[tokio::test]
async fn n_cmn_uni_9006_a_code_outside_the_common_codes_is_refused_on_every_family() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_unknown_unit_property_id = namespace.urn("Building", "unknown-unit-Property")?;
    let building_type = namespace.entity_type("Building")?;
    let building_unknown_unit_geo_property_id = namespace.urn("Building", "unknown-unit-GeoProperty")?;
    let building_unknown_unit_list_property_id = namespace.urn("Building", "unknown-unit-ListProperty")?;
    let building_unknown_unit_property = Entity::builder()
        .id(building_unknown_unit_property_id)
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (invalid unit code at measure), so `measure` is written as text.
    let measure: RawJson = r#"{"type": "Property", "value": 42, "unitCode": "NOTACODE"}"#.parse()?;
    let document = UncheckedDocument::from_entity(&building_unknown_unit_property, Lossless::Normalized).set_member("measure", &measure);
    let request = broker
        .unchecked(&CreateEntity::builder().entity(&building_unknown_unit_property).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let building_unknown_unit_geo_property = Entity::builder()
        .id(building_unknown_unit_geo_property_id)
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (invalid unit code at measure), so `measure` is written as text.
    let measure_2: RawJson = r#"{"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.0, 52.0]}, "unitCode": "NOTACODE"}"#.parse()?;
    let document_2 = UncheckedDocument::from_entity(&building_unknown_unit_geo_property, Lossless::Normalized).set_member("measure", &measure_2);
    let request_2 = broker
        .unchecked(&CreateEntity::builder().entity(&building_unknown_unit_geo_property).build())?
        .document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let building_unknown_unit_list_property = Entity::builder()
        .id(building_unknown_unit_list_property_id)
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (invalid unit code at measure), so `measure` is written as text.
    let measure_3: RawJson = r#"{"type": "ListProperty", "valueList": [300, 300, 120, 6], "unitCode": "NOTACODE"}"#.parse()?;
    let document_3 = UncheckedDocument::from_entity(&building_unknown_unit_list_property, Lossless::Normalized).set_member("measure", &measure_3);
    let request_3 = broker
        .unchecked(&CreateEntity::builder().entity(&building_unknown_unit_list_property).build())?
        .document(&document_3);
    let answer_3 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
