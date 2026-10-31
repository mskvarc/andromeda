//! Id takes precedence over id pattern in a registered entity info.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, identity_matcher::has_id, namespace::Namespace, nth::nth};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: an `EntityInfo` of a registration that gives both id and `idPattern` is matched by its id alone: a queried
/// id equal to it matches although the pattern does not, and a queried id the pattern matches but the id differs
/// from does not
/// Clause: 5.2.8, 5.2.33, 5.10.2, 5.12
#[gtest]
#[tokio::test]
async fn n_csr_qry_3001_id_takes_precedence_over_id_pattern_in_a_registered_entity_info() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_r1_id = namespace.registration_id("r1")?;
    let building_type = namespace.entity_type("Building")?;
    let building_x1_id = namespace.urn("Building", "x1")?;
    let registration_r2_id = namespace.registration_id("r2")?;
    let building_x2_id = namespace.urn("Building", "x2")?;
    let building_y1_id = namespace.urn("Building", "y1")?;
    let registration_r1 = CSourceRegistration::builder(source.endpoint("1")?)
        .id(registration_r1_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone())
                .with_id(building_x1_id.clone())
                .with_id_pattern(namespace.id_pattern_ending("Building", "zz.*")?)])
            .build()?])
        .build()?;
    broker.registrations().register(&registration_r1).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_r1_cleanup = Cleanup::of(&broker).registration(&registration_r1_id);
    let registration_r2 = CSourceRegistration::builder(source.endpoint("1")?)
        .id(registration_r2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone())
                .with_id(building_x2_id.clone())
                .with_id_pattern(namespace.id_pattern_ending("Building", "y.*")?)])
            .build()?])
        .build()?;
    broker.registrations().register(&registration_r2).send().await?;
    let _registration_r2_cleanup = Cleanup::of(&broker).registration(&registration_r2_id);
    // 5.2.8: "id takes precedence over idPattern": the queried id equals the registered id of r1, whose pattern does not match it
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_x1_id.clone()])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&registration_r1_id)));
    // the queried id matches the registered pattern of r2, but r2 registers the id x2, which takes precedence
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_y1_id])
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(0)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_x2_id.clone()])
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), nth(0, has_id(&registration_r2_id)));
    Ok(())
}
