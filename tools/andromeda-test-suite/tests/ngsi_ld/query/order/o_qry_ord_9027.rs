//! A registration for another type leaves the ordering admitted.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::{IdPattern, ordering::OrderBy},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: With a registration covering only another type, Query Entities with `orderBy=name` and no local parameter
/// answers 200 ascending by name
/// Clause: 4.23.1, 5.5.13, 5.7.2.4
/// Recorded: 5.7.2.4 and 5.7.4.4 refuse an ordering whose execution is not limited to the local scope (5.5.13),
/// which 5.5.13 defines as using no Context Source Registration; whether a query no registration matches is in that
/// state without local=true is left open. Andromeda admits the ordering when no registration takes part in the
/// query
#[gtest]
#[tokio::test]
async fn o_qry_ord_9027_a_registration_for_another_type_leaves_the_ordering_admitted() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let building_order_1_id = namespace.urn("Building", "order-1")?;
    let order_scope_type = namespace.entity_type("OrderScope")?;
    let building_order_2_id = namespace.urn("Building", "order-2")?;
    let building_order_3_id = namespace.urn("Building", "order-3")?;
    let registration_order_other_type_id = namespace.registration_id("order-other-type")?;
    let order_scope_elsewhere_type = namespace.entity_type("OrderScopeElsewhere")?;
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
    let registration_order_other_type = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_order_other_type_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(order_scope_elsewhere_type).with_id_pattern(format!("urn:ngsi-ld:.+:{namespace}-.*").parse::<IdPattern>()?)])
            .build()?])
        .operations([DistributedOperation::QueryEntity, DistributedOperation::QueryTemporal])
        .build()?;
    broker.registrations().register(&registration_order_other_type).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_order_other_type_cleanup = Cleanup::of(&broker).registration(&registration_order_other_type_id);
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&order_scope_type)
        .ordering("name".parse::<OrderBy>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(3)));
    expect_that!(page.items(), nth(0, has_id(&building_order_1_id)));
    expect_that!(page.items(), nth(1, has_id(&building_order_2_id)));
    expect_that!(page.items(), nth(2, has_id(&building_order_3_id)));
    broker.registrations().delete(&registration_order_other_type_id).send().await?;
    Ok(())
}
