//! A get query reaches a redirect query batch source as a batch query.

use andromeda_test_suite::{
    body_pattern::BodyPattern,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    identity_matcher::has_id,
    namespace::Namespace,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, Relationship},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities by type with only a redirect registration advertising `queryBatch` reaches the source
/// through one forwarded POST `/entityOperations/query`, never a forwarded GET, and answers the Entity the source
/// returns
/// Clause: 5.7.2.4, 4.20, 6.23.1, 4.3.6.1
#[gtest]
#[tokio::test]
async fn n_fed_qry_0005_a_get_query_reaches_a_redirect_query_batch_source_as_a_batch_query() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let off_street_parking_downtown1_id = namespace.urn("OffStreetParking", "Downtown1")?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .operations([DistributedOperation::QueryBatch])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let source_entities = [Entity::output_builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("brandName", "Mercedes")
        .relationship(
            "isParked",
            Relationship::builder(off_street_parking_downtown1_id).observed_at("2017-07-29T12:00:04Z".parse()?),
        )
        .build()?];
    source
        .answer_when(
            SourceRequest::new(Method::POST, "/a/ngsi-ld/v1/entityOperations/query")
                .without_query("offset")
                .body(BodyPattern::Contains(format!("Vehicle_{namespace}")))
                .priority(1),
            source_answer::entities(&source_entities),
        )
        .await?;
    let source_entities_2 = [];
    source
        .answer_when(
            SourceRequest::new(Method::POST, "/a/ngsi-ld/v1/entityOperations/query")
                .body(BodyPattern::Contains(format!("Vehicle_{namespace}")))
                .priority(2),
            source_answer::entities(&source_entities_2),
        )
        .await?;
    let page = broker.with_accept(Accept::any()).entities().query().entity_type(&vehicle_type).send().await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&vehicle_id)));
    let forwarded_request = SourceRequest::new(Method::POST, "/a/ngsi-ld/v1/entityOperations/query").without_query("offset");
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_request_2 = SourceRequest::matching(Method::GET, "/a/ngsi-ld/v1/(entities|entityOperations|temporal).*");
    let forwarded_2 = source.received_matching(&forwarded_request_2).await?;
    expect_that!(forwarded_2, len(eq(0)));
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
