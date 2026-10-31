//! A temporal instance sub attribute keeps no client system time.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, evolution_matcher::has_evolution, namespace::Namespace, nth::nth, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        UncheckedDocument,
        headers::Accept,
        operation::{CreateOrUpdateTemporalEvolution, SystemAttributes},
    },
    model::{Lossless, NgsiLdMediaType, RawJson, evolution::TemporalEntity},
};

/// Pins: Create Temporal Evolution of an instance whose sub-Attribute claims `createdAt` and `modifiedAt` answers
/// 201, and Retrieve Temporal Evolution under `sysAttrs` answers the sub-Attribute and carries neither claimed
/// instant anywhere
/// Clause: 5.2.5, 4.8, 5.6.11.4, 5.7.3.4
#[gtest]
#[tokio::test]
async fn n_cmn_sys_9007_a_temporal_instance_sub_attribute_keeps_no_client_system_time() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder().id(vehicle_id.clone()).entity_type(vehicle_type).build()?;
    // The model refuses it (member `createdAt` is not allowed here at name.accuracy), so `name` is written as text.
    let name: RawJson = r#"[{"type": "Property", "value": "first", "accuracy": {"type": "Property", "value": 1, "createdAt": "1900-01-01T00:00:00Z", "modifiedAt": "1900-01-01T00:00:00Z"}, "observedAt": "2024-01-01T00:00:00Z"}]"#.parse()?;
    let document = UncheckedDocument::try_from(vehicle_evolution.render(Lossless::Normalized))?.set_member("name", &name);
    let request = broker
        .unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&vehicle_evolution).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let received = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    expect_that!(String::from_utf8_lossy(&received.body), not(contains_substring("1900-01-01T00:00:00Z")));
    let retrieved = &received.outcome;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution("name", nth(0, any_attribute().sub_attribute("accuracy", any_attribute().value(1))))
    );
    broker.temporal().delete(&vehicle_id).send().await?;
    Ok(())
}
