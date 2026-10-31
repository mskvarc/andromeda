//! A posted query reaches a query entity only source as one get query.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, namespace::Namespace, source_answer, source_request::SourceRequest};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::operation::QueryMethod,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities on broker A through POST `entityOperations/query` under an inclusive registration
/// advertising only `queryEntity` reaches the source once as a GET query naming the type, and never as a POST
/// `entityOperations/query`
/// Clause: 5.7.2.4, 4.20, 4.3.6.1
#[gtest]
#[tokio::test]
async fn n_fed_qry_7601_a_posted_query_reaches_a_query_entity_only_source_as_one_get_query() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let source_entities = [];
    source
        .answer(Method::GET, "/ngsi-ld/v1/entities", source_answer::entities(&source_entities))
        .await?;
    let registration = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    broker
        .entities()
        .query()
        .method(QueryMethod::Post)
        .id_pattern(namespace.id_pattern("Vehicle")?)
        .entity_type(&vehicle_type)
        .send()
        .await?;
    let forwarded_request = SourceRequest::new(Method::GET, "/ngsi-ld/v1/entities").query("type", format!("Vehicle_{namespace}"));
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_request_2 = SourceRequest::any_matching("/ngsi-ld/v1/entityOperations/.*");
    let forwarded_2 = source.received_matching(&forwarded_request_2).await?;
    expect_that!(forwarded_2, len(eq(0)));
    Ok(())
}
