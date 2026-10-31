//! An exclusive block of two ids is queried by those ids.

use andromeda_test_suite::{
    body_pattern::BodyPattern,
    broker,
    context_source::ContextSource,
    namespace::Namespace,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::headers::Accept,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities by type reaches an exclusive registration advertising only `queryBatch`, whose one
/// information block names two Entity ids, through one forwarded batch query with one selector per covered id
/// Clause: 5.7.2.4, 4.3.6.3, 4.20, 6.23.1
/// Recorded: clause 4.3.6.1 requires a forwarded request to respect the registration constraints but does not fix
/// how a type-only query is narrowed to the ids of an exclusive block; Andromeda names one selector per covered id
#[gtest]
#[tokio::test]
async fn o_fed_qry_7412_an_exclusive_block_of_two_ids_is_queried_by_those_ids() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id)
        .information([RegistrationInfo::builder()
            .entities([
                EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone()),
                EntityInfo::new(vehicle_type.clone()).with_id(vehicle_2_id.clone()),
            ])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Exclusive)
        .operations([DistributedOperation::QueryBatch])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let source_entities = [];
    source
        .answer_when(
            SourceRequest::new(Method::POST, "/a/ngsi-ld/v1/entityOperations/query")
                .without_query("offset")
                .priority(1),
            source_answer::entities(&source_entities),
        )
        .await?;
    source
        .answer_when(
            SourceRequest::new(Method::POST, "/a/ngsi-ld/v1/entityOperations/query").priority(2),
            source_answer::entities(&source_entities),
        )
        .await?;
    let page = broker.with_accept(Accept::any()).entities().query().entity_type(&vehicle_type).send().await?;
    expect_that!(page.items(), is_empty());
    let forwarded_request = SourceRequest::new(Method::POST, "/a/ngsi-ld/v1/entityOperations/query").without_query("offset");
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(1)));
    let covered_request = forwarded_request
        .body(BodyPattern::Contains(vehicle_id.to_string()))
        .body(BodyPattern::Contains(vehicle_2_id.to_string()));
    let covered = source.received_matching(&covered_request).await?;
    expect_that!(covered, len(eq(1)));
    let forwarded_request_2 = SourceRequest::matching(Method::GET, "/a/.*");
    let forwarded_2 = source.received_matching(&forwarded_request_2).await?;
    expect_that!(forwarded_2, len(eq(0)));
    Ok(())
}
