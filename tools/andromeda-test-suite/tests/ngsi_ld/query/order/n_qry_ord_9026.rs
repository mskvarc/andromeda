//! A registration taking part in the query refuses the ordering.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::SuiteConfig,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
    query::{
        IdPattern,
        Locality,
        ordering::OrderBy,
        temporal_query::{self, TemporalProperty},
    },
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: With a registration covering the queried type, Query Entities with `orderBy` and Query Temporal Evolution
/// of Entities with `orderBy=id` answer 400 `BadRequestData`, and Query Entities with local=true and `orderBy=name`
/// answers 200 ascending by name
/// Clause: 4.23.1, 5.5.13, 5.7.2.4, 5.7.4.4
#[gtest]
#[tokio::test]
async fn n_qry_ord_9026_a_registration_taking_part_in_the_query_refuses_the_ordering() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let building_order_1_id = namespace.urn("Building", "order-1")?;
    let order_scope_type = namespace.entity_type("OrderScope")?;
    let building_order_2_id = namespace.urn("Building", "order-2")?;
    let building_order_3_id = namespace.urn("Building", "order-3")?;
    let registration_order_matching_id = namespace.registration_id("order-matching")?;
    let building_order_1 = Entity::builder()
        .id(building_order_1_id.clone())
        .entity_type(order_scope_type.clone())
        .property("name", "A")
        .build()?;
    broker.entities().create(&building_order_1).send().await?.done()?;
    let building_order_2 = Entity::builder()
        .id(building_order_2_id.clone())
        .entity_type(order_scope_type.clone())
        .property("name", "B")
        .build()?;
    broker.entities().create(&building_order_2).send().await?.done()?;
    let building_order_3 = Entity::builder()
        .id(building_order_3_id.clone())
        .entity_type(order_scope_type.clone())
        .property("name", "C")
        .build()?;
    broker.entities().create(&building_order_3).send().await?.done()?;
    let registration_order_matching = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_order_matching_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(order_scope_type.clone()).with_id_pattern(format!("urn:ngsi-ld:.+:{namespace}-.*").parse::<IdPattern>()?)])
            .build()?])
        .operations([DistributedOperation::QueryEntity, DistributedOperation::QueryTemporal])
        .build()?;
    broker.registrations().register(&registration_order_matching).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_order_matching_cleanup = Cleanup::of(&broker).registration(&registration_order_matching_id);
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&order_scope_type)
        .ordering("name".parse::<OrderBy>()?)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("1970-01-01T00:00:00Z".parse()?).time_property(TemporalProperty::CreatedAt))
        .entity_type(&order_scope_type)
        .ordering("id".parse::<OrderBy>()?)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&order_scope_type)
        .locality(Locality::Local)
        .ordering("name".parse::<OrderBy>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(3)));
    expect_that!(page.items(), nth(0, has_id(&building_order_1_id)));
    expect_that!(page.items(), nth(1, has_id(&building_order_2_id)));
    expect_that!(page.items(), nth(2, has_id(&building_order_3_id)));
    broker.registrations().delete(&registration_order_matching_id).send().await?;
    Ok(())
}
