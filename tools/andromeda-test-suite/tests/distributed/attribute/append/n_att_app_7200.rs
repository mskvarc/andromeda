//! An attribute of an unknown attribute type is bad request data and appends nothing.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    outcome::Done,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::AppendAttributes},
    model::{Entity, EntityFragment, ErrorType, Lossless, RawJson},
    query::Locality,
};

/// Pins: Append Entity Attributes with an attribute whose type is not an NGSI-LD attribute type answers 400
/// `BadRequestData` and the Entity is unchanged
/// Clause: 5.6.3.4, 5.2.5, 6.6.3.1
///
/// Invalid type: `NotAnAttributeType` (the subject is an attribute type no NGSI-LD attribute has)
#[gtest]
#[tokio::test]
async fn n_att_app_7200_an_attribute_of_an_unknown_attribute_type_is_bad_request_data_and_appends_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 5)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let fragment = EntityFragment::builder().build()?;
    // The model refuses it (unknown attribute type `NotAnAttributeType` at fuel), so `fuel` is written as text.
    let fuel: RawJson = r#"{"type": "NotAnAttributeType", "value": 1}"#.parse()?;
    let document = UncheckedDocument::try_from(fragment.render(Lossless::Normalized))?.set_member("fuel", &fuel);
    let request = broker
        .unchecked(&AppendAttributes::builder().id(&vehicle_id).fragment(&fragment).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved = broker
        .with_accept(Accept::any())
        .entities()
        .retrieve(&vehicle_id)
        .locality(Locality::Local)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", property().value(5)));
    let retrieved_2 = broker
        .with_accept(Accept::any())
        .entities()
        .retrieve(&vehicle_id)
        .locality(Locality::Local)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2, lacks_attribute("fuel"));
    Ok(())
}
