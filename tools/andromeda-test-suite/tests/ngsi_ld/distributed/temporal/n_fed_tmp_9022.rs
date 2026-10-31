//! A forwarded temporal retrieve carries only its own parameters.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    evolution_matcher::has_evolution,
    forwarded_query::query_values,
    namespace::Namespace,
    nth::nth,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Retrieve Temporal Evolution with attrs and a before window, under an inclusive registration selecting the
/// type and an id pattern, answers 200 with the source's history and forwards one retrieval carrying timerel,
/// `timeAt` and attrs, and no id, type, `idPattern` or limit
/// Clause: 5.7.3.4, 4.3.6.1, Table 6.19.3.1-1, 6.3.20
#[gtest]
#[tokio::test]
async fn n_fed_tmp_9022_a_forwarded_temporal_retrieve_carries_only_its_own_parameters() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::RetrieveTemporal])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let source_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("fuel", Property::builder(70).observed_at("2024-01-01T00:00:00Z".parse()?))
        .build()?;
    source
        .answer_when(
            SourceRequest::matching(Method::GET, "/a/ngsi-ld/v1/temporal/entities/[^/]+"),
            source_answer::temporal_entity(&source_evolution),
        )
        .await?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .temporal_q(temporal_query::before("2030-01-01T00:00:00Z".parse()?))
        .attrs(vec!["fuel".parse()?])
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(evolution.id, eq(&vehicle_id));
    expect_that!(evolution, has_evolution("fuel", nth(0, any_attribute().value(70))));
    let forwarded_request = SourceRequest::matching(Method::GET, "/a/ngsi-ld/v1/temporal/entities/[^/]+");
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_one = forwarded.first().or_fail()?;
    expect_that!(query_values(forwarded_one, "timerel"), nth(0, eq("before")));
    expect_that!(query_values(forwarded_one, "timeAt"), nth(0, eq("2030-01-01T00:00:00Z")));
    expect_that!(query_values(forwarded_one, "attrs"), nth(0, eq("fuel")));
    expect_that!(query_values(forwarded_one, "id"), is_empty());
    expect_that!(query_values(forwarded_one, "type"), is_empty());
    expect_that!(query_values(forwarded_one, "idPattern"), is_empty());
    expect_that!(query_values(forwarded_one, "limit"), is_empty());
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
