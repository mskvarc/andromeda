//! A create naming the NGSI-LD Null beside another type is bad request data.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateEntity},
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: Create Entity with type `["Vehicle", "urn:ngsi-ld:null"]` answers 400 `BadRequestData` and creates nothing
/// Clause: 5.5.4, 5.6.1.4
#[gtest]
#[tokio::test]
async fn n_ent_cre_9068_a_create_naming_the_null_beside_another_type_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", "null-beside")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 50)
        .build()?;
    // The model holds type names, so the Null is written as text.
    let types: RawJson = format!(r#"["{vehicle_type}", "urn:ngsi-ld:null"]"#).parse()?;
    let document = UncheckedDocument::from_entity(&vehicle, Lossless::Normalized).set_member("type", &types);
    let request = broker.unchecked(&CreateEntity::builder().entity(&vehicle).build())?.document(&document);
    expect_that!(
        &broker.send_unchecked(request).await,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
    );
    let retrieved = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await;
    expect_that!(&retrieved, answered(StatusCode::NOT_FOUND));
    Ok(())
}
