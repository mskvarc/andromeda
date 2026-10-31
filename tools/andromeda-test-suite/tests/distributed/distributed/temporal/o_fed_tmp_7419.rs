//! A source is asked for normalized instances.

use andromeda_test_suite::{body_json::json_exists, broker, context_source::ContextSource, forwarded_query::query_values, namespace::Namespace, source_answer};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{RepresentationSpelling, RetrieveTemporalEvolution, SimplifiedTemporal, TemporalSelection},
    },
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query::TemporalQuery,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Retrieve Temporal Evolution as `temporalValues` forwards to the source a retrieval naming neither format
/// nor options `temporalValues`
/// Clause: 5.7.3.4, 4.5.9
/// Recorded: the clause does not say which representation a forwarded retrieval asks for; Andromeda asks the source
/// for the normalized instances so that the instances of one dataset can be merged
#[gtest]
#[tokio::test]
async fn o_fed_tmp_7419_a_source_is_asked_for_normalized_instances() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .property_names(["speed"])
            .build()?])
        .operations([DistributedOperation::RetrieveTemporal])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let source_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property(
            "speed",
            Property::builder(10)
                .dataset_id(namespace.dataset_id("Dataset", "a")?)
                .observed_at("2020-08-01T12:00:00Z".parse()?),
        )
        .build()?;
    let source_path = format!("/a/ngsi-ld/v1/temporal/entities/{vehicle_id}");
    source
        .answer(Method::GET, &source_path, source_answer::temporal_entity(&source_evolution))
        .await?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let before = TemporalQuery::before("2030-01-01T00:00:00Z".parse()?);
    // The simplified temporal reader refuses the `datasetId` this answer carries, so the answer is read as received.
    for spelling in [RepresentationSpelling::default(), RepresentationSpelling::default().in_options()] {
        let retrieve = RetrieveTemporalEvolution::builder()
            .id(&vehicle_id)
            .selection(TemporalSelection::Query(&before))
            .format(SimplifiedTemporal)
            .spelling(spelling)
            .build();
        let response = json.send_unchecked(json.unchecked(&retrieve)?).await?;
        expect_that!(response.status(), eq(StatusCode::OK));
        expect_that!(response.body(), json_exists("$.speed"));
    }
    let forwarded = source.received_on(Method::GET, &source_path).await?;
    expect_that!(forwarded, len(eq(2)));
    for forwarded_one in &forwarded {
        expect_that!(query_values(forwarded_one, "format"), is_empty());
        expect_that!(query_values(forwarded_one, "options"), is_empty());
    }
    Ok(())
}
