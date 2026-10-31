//! No empty fragment records a temporal instance.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    problem::answered,
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType},
    query::temporal_query::{self, TemporalProperty},
};

/// Pins: Partial Attribute Update, Update Attributes, Append Attributes and Merge Entity with the empty fragment {}
/// each answer 204, and the Temporal Evolution of the Entity still holds one instance of speed and of `brandName`,
/// those its creation recorded
/// Clause: 4.3.1, 4.5.7, 5.6.2.4, 5.6.3.4, 5.6.4.4, 5.6.17.4, 5.7.3
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default
#[gtest]
#[tokio::test]
async fn o_cmn_val_9031_no_empty_fragment_records_a_temporal_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 50)
        .property("brandName", "Mercedes")
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .temporal_q(temporal_query::after("2000-01-01T00:00:00Z".parse()?).time_property(TemporalProperty::ModifiedAt))
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(evolution, has_evolution("speed", all!(len(eq(1)), nth(0, any_attribute().value(50)))));
    expect_that!(
        evolution,
        has_evolution("brandName", all!(len(eq(1)), nth(0, any_attribute().value("Mercedes"))))
    );
    let empty = EntityFragment::builder().build()?;
    // The empty fragment reads in the concise form as a Property holding an empty object, which is not what the
    // test sends, so the bytes are sent as written.
    let request = request_to(&broker, Method::PATCH, &format!("entities/{vehicle_id}/attrs/speed"))?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from_static(b"{}"));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    broker.entities().update_attributes(&vehicle_id, &empty).send().await?.done()?;
    broker.entities().append_attributes(&vehicle_id, &empty).send().await?.done()?;
    broker.entities().merge(&vehicle_id, &empty).send().await?.done()?;
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .temporal_q(temporal_query::after("2000-01-01T00:00:00Z".parse()?).time_property(TemporalProperty::ModifiedAt))
        .send()
        .await?;
    let evolution_2 = retrieved_2.body();
    expect_that!(evolution_2, has_evolution("speed", all!(len(eq(1)), nth(0, any_attribute().value(50)))));
    expect_that!(
        evolution_2,
        has_evolution("brandName", all!(len(eq(1)), nth(0, any_attribute().value("Mercedes"))))
    );
    Ok(())
}
