//! A temporal write repeating an instanceId is refused alone.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    problem::{answered, problem},
    unchecked_document::from_rendered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateOrUpdateTemporalEvolution},
    model::{ErrorType, Input, InstanceId, Lossless, NgsiLdMediaType, Property, RawJson, evolution::TemporalEntity},
};

/// Pins: Create or Update Temporal Evolution of an Entity answers 400 `BadRequestData` for an Entity whose `reading`
/// gives one `instanceId` to the instances of two `datasetId`s and stores nothing for it, while the two Entities sent
/// at the same moment are created
/// Clause: 4.5.7, 5.5.4, 5.6.11.4
#[gtest]
#[tokio::test]
async fn n_tmp_ups_9094_a_temporal_write_repeating_an_instance_id_is_refused_alone() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let meter_type = namespace.entity_type("Meter")?;
    let [repeating_id, b_id, c_id] = ["a", "b", "c"].map(|suffix| namespace.urn("Meter", suffix));
    let (repeating_id, b_id, c_id) = (repeating_id?, b_id?, c_id?);
    let instance_id: InstanceId = format!("urn:ngsi-ld:Instance:{namespace}-r").parse()?;
    let repeating = TemporalEntity::builder()
        .id(repeating_id.clone())
        .entity_type(meter_type.clone())
        .identified(
            "reading",
            instance_id.clone(),
            Property::builder(1).observed_at("2024-01-01T01:00:00Z".parse()?),
        )
        .build()?;
    // Clause 4.5.7: an instanceId identifies one instance. The model refuses two instances sharing one, so the
    // `reading` evolution is set as text on the rendered document.
    let repeated_instances: RawJson = format!(
        concat!(
            r#"[{{"type": "Property", "value": 1, "observedAt": "2024-01-01T01:00:00Z", "datasetId": "{x}", "instanceId": "{instance}"}},"#,
            r#"{{"type": "Property", "value": 2, "observedAt": "2024-01-01T02:00:00Z", "datasetId": "{y}", "instanceId": "{instance}"}}]"#
        ),
        x = namespace.dataset_id("Dataset", "x")?,
        y = namespace.dataset_id("Dataset", "y")?,
        instance = instance_id,
    )
    .parse()?;
    let document: UncheckedDocument = from_rendered(&repeating.render(Lossless::Normalized))?.set_member("reading", &repeated_instances);
    let repeating_request = broker
        .unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&repeating).build())?
        .document(&document);
    let single = |id| -> Result<TemporalEntity<Input>> {
        Ok(TemporalEntity::builder()
            .id(id)
            .entity_type(meter_type.clone())
            .identified(
                "reading",
                instance_id.clone(),
                Property::builder(5).observed_at("2024-01-01T01:00:00Z".parse()?),
            )
            .build()?)
    };
    let (b, c) = (single(b_id.clone())?, single(c_id.clone())?);
    let responding = broker.with_responses();
    let temporal = responding.temporal();
    // Sent together so the broker can commit the three writes in one transaction.
    let (refused, created_b, created_c) = tokio::join!(
        broker.send_unchecked(repeating_request),
        temporal.create_or_update(&b).send(),
        temporal.create_or_update(&c).send(),
    );
    expect_that!(&refused, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    expect_that!(&created_b, answered(StatusCode::CREATED));
    expect_that!(&created_c, answered(StatusCode::CREATED));
    let absent = broker.with_responses().temporal().retrieve(&repeating_id).send().await;
    expect_that!(&absent, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    // The same identifier on another Entity names another instance.
    for id in [&b_id, &c_id] {
        let retrieved = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).temporal().retrieve(id).send().await?;
        expect_that!(
            retrieved.body(),
            has_evolution("reading", all!(len(eq(1)), contains(any_attribute().instance_id(&instance_id).value(5))))
        );
    }
    Ok(())
}
