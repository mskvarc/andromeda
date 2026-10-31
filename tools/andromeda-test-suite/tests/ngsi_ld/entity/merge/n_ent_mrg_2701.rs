//! A concise empty object attribute is merged as its value.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::has_attribute,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    system_time::{self, SystemMember},
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use indexmap::IndexMap;
use ngsi_ld::{
    http::{headers::Accept, operation::SystemAttributes},
    model::{Entity, NgsiLdMediaType, Value},
    query::temporal_query::TemporalProperty,
};

/// Pins: Merge Entity whose Fragment holds an Attribute as the empty object {} merges it as a concise Property
/// whose value is that object, keeps the stored type, moves `modifiedAt` and records one new instance in the
/// Temporal Evolution
/// Clause: 3.1, 4.5.2.3, 4.8, 5.2.5, 5.6.17
/// Recorded: an object without "type" is a concise Property Value (clause 4.5.2.3), the empty object is a JSON
/// value (clause 3.1), and a merged simplified Attribute keeps the stored type (clause 5.6.17.4)
#[gtest]
#[tokio::test]
async fn n_ent_mrg_2701_a_concise_empty_object_attribute_is_merged_as_its_value() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 55)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity = retrieved.body();
    let speed_modified = system_time::of_attribute(entity, &["speed"], SystemMember::ModifiedAt)?;
    // The concise `{}` is a Property whose value is the empty object; the model writes that attribute in its
    // normalized spelling, so the fragment is sent as written.
    let request = request_to(&broker, Method::PATCH, &format!("entities/{vehicle_id}"))?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from_static(b"{ \"speed\": {} }"));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(
        entity_2,
        has_attribute("speed", property().value(Value::Object(IndexMap::new())).length_at([], 0))
    );
    expect_that!(
        system_time::of_attribute(entity_2, &["speed"], SystemMember::ModifiedAt)?,
        not(eq(speed_modified))
    );
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .time_property(TemporalProperty::ModifiedAt)
        .send()
        .await?;
    let evolution = retrieved_3.body();
    expect_that!(evolution, has_evolution("speed", len(eq(2))));
    Ok(())
}
