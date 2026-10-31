//! A query reaches a query batch source as a batch query naming the entity.

use andromeda_test_suite::{
    body_pattern::BodyPattern,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    namespace::Namespace,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Query Entities by type and id, by GET and by POST, over an inclusive registration advertising only
/// `queryBatch` is forwarded as a POST batch query whose body is a Query naming the Entity by id and type, never as
/// a GET
/// Clause: 4.3.6.1, 4.20, 6.23.1, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_fed_qry_7417_a_query_reaches_a_query_batch_source_as_a_batch_query_naming_the_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .operations([DistributedOperation::QueryBatch])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let source_entities = [];
    source
        .answer_when(
            SourceRequest::new(Method::POST, "/a/ngsi-ld/v1/entityOperations/query")
                .without_query("offset")
                .priority(1),
            source_answer::entities(&source_entities),
        )
        .await?;
    let source_entities_2 = [];
    source
        .answer_when(
            SourceRequest::new(Method::POST, "/a/ngsi-ld/v1/entityOperations/query").priority(2),
            source_answer::entities(&source_entities_2),
        )
        .await?;
    let page = broker
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .ids(vec![vehicle_id])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(0)));
    let page_2 = broker
        .entities()
        .query()
        .method(QueryMethod::Post)
        .ids(vec![vehicle_2_id])
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(0)));
    let forwarded_request = SourceRequest::new(Method::POST, "/a/ngsi-ld/v1/entityOperations/query")
        .without_query("offset")
        .body(BodyPattern::Selects("$[?(@.type == 'Query')]".parse()?))
        .body(BodyPattern::SelectsBelow(
            "$.entities".parse()?,
            format!("$[?(@.id == 'urn:ngsi-ld:Vehicle:{namespace}-1' && @.type == 'Vehicle_{namespace}')]").parse()?,
        ));
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_request_2 = SourceRequest::new(Method::POST, "/a/ngsi-ld/v1/entityOperations/query")
        .without_query("offset")
        .body(BodyPattern::Selects("$[?(@.type == 'Query')]".parse()?))
        .body(BodyPattern::SelectsBelow(
            "$.entities".parse()?,
            format!("$[?(@.id == 'urn:ngsi-ld:Vehicle:{namespace}-2' && @.type == 'Vehicle_{namespace}')]").parse()?,
        ));
    let forwarded_2 = source.received_matching(&forwarded_request_2).await?;
    expect_that!(forwarded_2, len(eq(1)));
    let forwarded_request_3 = SourceRequest::matching(Method::GET, "/.*");
    let forwarded_3 = source.received_matching(&forwarded_request_3).await?;
    expect_that!(forwarded_3, len(eq(0)));
    Ok(())
}
