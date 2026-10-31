//! Deleting expires at records no attribute history.

use andromeda_test_suite::{
    broker,
    evolution_matcher::{has_evolution, lacks_evolution},
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    wire::api_uri,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::DeleteAttribute},
    model::{AttributeName, Entity, NgsiLdMediaType},
    query::temporal_query::TemporalProperty,
};

/// Pins: Delete Attribute naming expiresAt removes the member, and the Temporal Evolution of the entity holds no
/// expiresAt attribute afterwards
/// Clause: 4.22, 4.5.7, 5.6.5
/// Recorded: 4.22 makes expiresAt a member of the Entity, not an Attribute, so it has no Temporal Evolution to record
/// a deletion in
#[gtest]
#[tokio::test]
async fn n_att_del_4001_deleting_expires_at_records_no_attribute_history() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .expires_at("2099-01-01T00:00:00Z".parse()?)
        .property("name", "Tower")
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    // The model refuses `expiresAt` as an attribute name, so the operation is built for a placeholder and sent to the path as written.
    let request = broker
        .unchecked(
            &DeleteAttribute::builder()
                .id(&building_id)
                .name(&"placeholder".parse::<AttributeName>()?)
                .build(),
        )?
        .uri(api_uri(&broker, &format!("entities/{building_id}/attrs/expiresAt"))?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    expect_that!(retrieved.body().expires_at, none());
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&building_id)
        .time_property(TemporalProperty::ModifiedAt)
        .send()
        .await?;
    let evolution = retrieved_2.body();
    expect_that!(evolution, has_evolution("name", len(eq(1))));
    expect_that!(evolution.expires_at, none());
    expect_that!(evolution, lacks_evolution("expiresAt"));
    expect_that!(evolution, lacks_evolution("ngsi-ld:expiresAt"));
    Ok(())
}
