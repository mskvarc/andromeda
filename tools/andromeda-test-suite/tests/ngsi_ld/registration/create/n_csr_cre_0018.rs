//! A registration reads back every information member as written.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::SuiteConfig,
    namespace::Namespace,
    spelling_matcher::{listing_at, listing_count},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, OneOrMany},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Create Context Source Registration with two information items, the first with a single-type `EntityInfo`
/// and an `EntityInfo` of two types with an `idPattern` plus `propertyNames`, the second with `relationshipNames`
/// only, and Retrieve Context Source Registration answers both items in order, the single type as a string, the two
/// types as an array in written order, the pattern, and each name list on its own item only
/// Clause: 5.2.8, 5.2.9, 5.2.10
/// Tenant: private (its registration names no entity selector and matches every query of the tenant)
#[gtest]
#[tokio::test]
async fn n_csr_cre_0018_a_registration_reads_back_every_information_member_as_written() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let config = SuiteConfig::get()?;
    let registration_id = namespace.registration_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let shop_type = namespace.entity_type("Shop")?;
    let bank_type = namespace.entity_type("Bank")?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([
            RegistrationInfo::builder()
                .entities([
                    EntityInfo::new(building_type.clone()),
                    EntityInfo::new(OneOrMany::try_from(vec![namespace.entity_type("Shop")?, namespace.entity_type("Bank")?])?)
                        .with_id_pattern(namespace.id_pattern("Shop")?),
                ])
                .property_names([namespace.attribute("p")?.to_string()])
                .build()?,
            RegistrationInfo::builder()
                .relationship_names([namespace.attribute("r")?.to_string()])
                .build()?,
        ])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let registration_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&registration_id)
        .send()
        .await?;
    expect_that!(registration_2.information(), len(eq(2)));
    expect_that!(registration_2.information().first().map(|info| info.entities().len()), some(eq(2)));
    expect_that!(
        registration_2
            .information()
            .first()
            .and_then(|info| info.entities().first())
            .map(EntityInfo::entity_type)
            .and_then(OneOrMany::as_one),
        some(anything())
    );
    expect_that!(
        registration_2
            .information()
            .first()
            .and_then(|info| info.entities().first())
            .map(EntityInfo::entity_type),
        some(eq(&OneOrMany::one(building_type.clone())))
    );
    expect_that!(
        registration_2
            .information()
            .first()
            .and_then(|info| info.entities().first())
            .and_then(EntityInfo::id),
        none()
    );
    expect_that!(
        registration_2
            .information()
            .first()
            .and_then(|info| info.entities().first())
            .and_then(EntityInfo::id_pattern),
        none()
    );
    expect_that!(
        registration_2
            .information()
            .first()
            .and_then(|info| info.entities().get(1))
            .map(EntityInfo::entity_type)
            .is_some_and(OneOrMany::is_many),
        eq(true)
    );
    expect_that!(
        registration_2
            .information()
            .first()
            .and_then(|info| info.entities().get(1))
            .map(EntityInfo::entity_type),
        some(listing_count(2))
    );
    expect_that!(
        registration_2
            .information()
            .first()
            .and_then(|info| info.entities().get(1))
            .map(EntityInfo::entity_type),
        some(listing_at(0, &shop_type))
    );
    expect_that!(
        registration_2
            .information()
            .first()
            .and_then(|info| info.entities().get(1))
            .map(EntityInfo::entity_type),
        some(listing_at(1, &bank_type))
    );
    expect_that!(
        registration_2
            .information()
            .first()
            .and_then(|info| info.entities().get(1))
            .and_then(EntityInfo::id_pattern)
            .map(ToString::to_string)
            .as_deref(),
        some(eq(format!("urn:ngsi-ld:Shop:{namespace}-.*").as_str()))
    );
    expect_that!(
        registration_2
            .information()
            .first()
            .and_then(|info| info.entities().get(1))
            .and_then(EntityInfo::id),
        none()
    );
    expect_that!(registration_2.information().first().map(|info| info.property_names().len()), some(eq(1)));
    expect_that!(
        registration_2
            .information()
            .first()
            .and_then(|info| info.property_names().first())
            .map(ToString::to_string)
            .as_deref(),
        some(eq(format!("p_{namespace}").as_str()))
    );
    expect_that!(
        registration_2.information().first().map(|info| info.relationship_names().is_empty()),
        some(eq(true))
    );
    expect_that!(registration_2.information().get(1).map(|info| info.entities().is_empty()), some(eq(true)));
    expect_that!(registration_2.information().get(1).map(|info| info.property_names().is_empty()), some(eq(true)));
    expect_that!(registration_2.information().get(1).map(|info| info.relationship_names().len()), some(eq(1)));
    expect_that!(
        registration_2
            .information()
            .get(1)
            .and_then(|info| info.relationship_names().first())
            .map(ToString::to_string)
            .as_deref(),
        some(eq(format!("r_{namespace}").as_str()))
    );
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
