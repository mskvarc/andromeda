//! Dataset id none deletes only the default instance.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, has_instances},
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    wire::api_uri,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::DeleteAttribute},
    model::{AttributeName, Entity, NgsiLdMediaType, Property},
};

/// Pins: Delete Attribute with the datasetId parameter "@none" deletes the default instance only and keeps the named
/// instance
/// Clause: 4.5.5, 5.6.5
#[gtest]
#[tokio::test]
async fn n_att_del_2301_dataset_id_none_deletes_only_the_default_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let gps_dataset = namespace.dataset_id("Dataset", "gps")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 1)
        .property("speed", Property::builder(2).dataset_id(namespace.dataset_id("Dataset", "gps")?))
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    // `@none` is not a dataset id the operation can state, so the operation is sent to the path with the parameter as written.
    let request = broker
        .unchecked(&DeleteAttribute::builder().id(&vehicle_id).name(&"speed".parse::<AttributeName>()?).build())?
        .uri(api_uri(&broker, &format!("entities/{vehicle_id}/attrs/speed?datasetId=@none"))?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_instances("speed", 1));
    expect_that!(entity, has_attribute("speed", any_attribute().dataset_id(gps_dataset).value(2)));
    Ok(())
}
