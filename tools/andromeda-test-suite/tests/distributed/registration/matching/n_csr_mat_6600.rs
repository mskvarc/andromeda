//! The entity specification is matched against entity info by type id and id pattern.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, identity_matcher::has_id, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{EntityTypeName, NgsiLdMediaType},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Query Context Source Registrations by type with an id, an `idPattern` or an attrs, over registrations
/// whose `EntityInfo` holds an id, an `idPattern`, both or neither, answer 200 with the registration the matching
/// rules of clause 5.12 admit and without the others
/// Clause: 5.10.2.4, 5.12
/// Tenant: private (a type-only `EntityInfo` and a registration without `EntityInfo` match every id-only request or
/// every attrs-only query of the tenant (clause 5.12), which in the shared tenant would capture the requests of
/// other files; clause 4.14 keeps each tenant's registrations its own)
#[gtest]
#[tokio::test]
async fn n_csr_mat_6600_the_entity_specification_is_matched_against_entity_info_by_type_id_and_id_pattern() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let tenant = broker.with_tenant(namespace.tenant()?);
    let registration_provision_id = namespace.registration_id("provision")?;
    let sentinel_type = namespace.entity_type("Sentinel")?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_covered_id = namespace.urn("Vehicle", "covered")?;
    let registration_2_id = namespace.registration_id(2)?;
    let registration_3_id = namespace.registration_id(3)?;
    let registration_4_id = namespace.registration_id(4)?;
    let registration_5_id = namespace.registration_id(5)?;
    let registration_6_id = namespace.registration_id(6)?;
    let fleet_covered_id = namespace.urn("Fleet", "covered")?;
    let registration_7_id = namespace.registration_id(7)?;
    let registration_8_id = namespace.registration_id(8)?;
    let vehicle_other_id = namespace.urn("Vehicle", "other")?;
    let vehicle_anything_id = namespace.urn("Vehicle", "anything")?;
    let fleet_id = namespace.urn("Fleet", 1)?;
    let other_id = namespace.urn("Other", 1)?;
    let registration_provision = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_provision_id.clone())
        .information([RegistrationInfo::builder().entities([sentinel_type.as_str()]).build()?])
        .build()?;
    tenant.registrations().register(&registration_provision).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_provision_cleanup = Cleanup::of(&tenant).registration(&registration_provision_id);
    tenant.registrations().delete(&registration_provision_id).send().await?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(format!("https://test.andromeda/{namespace}/T1").parse::<EntityTypeName>()?).with_id(vehicle_covered_id.clone())])
            .property_names(["speed"])
            .build()?])
        .build()?;
    tenant.registrations().register(&registration).send().await?;
    let _registration_cleanup = Cleanup::of(&tenant).registration(&registration_id);
    let registration_2 = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_2_id.clone())
        .information([
            RegistrationInfo::builder()
                .entities([format!("https://test.andromeda/{namespace}/T2a")])
                .build()?,
            RegistrationInfo::builder()
                .entities([format!("https://test.andromeda/{namespace}/T2b")])
                .build()?,
        ])
        .build()?;
    tenant.registrations().register(&registration_2).send().await?;
    let _registration_2_cleanup = Cleanup::of(&tenant).registration(&registration_2_id);
    let registration_3 = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_3_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(format!("https://test.andromeda/{namespace}/T3").parse::<EntityTypeName>()?).with_id(vehicle_covered_id.clone())])
            .build()?])
        .build()?;
    tenant.registrations().register(&registration_3).send().await?;
    let _registration_3_cleanup = Cleanup::of(&tenant).registration(&registration_3_id);
    let registration_4 = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_4_id.clone())
        .information([RegistrationInfo::builder()
            .entities([format!("https://test.andromeda/{namespace}/T4")])
            .build()?])
        .build()?;
    tenant.registrations().register(&registration_4).send().await?;
    let _registration_4_cleanup = Cleanup::of(&tenant).registration(&registration_4_id);
    let registration_5 = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_5_id.clone())
        .information([RegistrationInfo::builder()
            .entities([
                EntityInfo::new(format!("https://test.andromeda/{namespace}/T5").parse::<EntityTypeName>()?).with_id_pattern(namespace.id_pattern("Fleet")?)
            ])
            .build()?])
        .build()?;
    tenant.registrations().register(&registration_5).send().await?;
    let _registration_5_cleanup = Cleanup::of(&tenant).registration(&registration_5_id);
    let registration_6 = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_6_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(format!("https://test.andromeda/{namespace}/T6").parse::<EntityTypeName>()?).with_id(fleet_covered_id)])
            .build()?])
        .build()?;
    tenant.registrations().register(&registration_6).send().await?;
    let _registration_6_cleanup = Cleanup::of(&tenant).registration(&registration_6_id);
    let registration_7 = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_7_id.clone())
        .information([RegistrationInfo::builder()
            .entities([
                EntityInfo::new(format!("https://test.andromeda/{namespace}/T7").parse::<EntityTypeName>()?).with_id_pattern(namespace.id_pattern("Fleet")?)
            ])
            .build()?])
        .build()?;
    tenant.registrations().register(&registration_7).send().await?;
    let _registration_7_cleanup = Cleanup::of(&tenant).registration(&registration_7_id);
    let registration_8 = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_8_id.clone())
        .information([RegistrationInfo::builder().property_names([namespace.attribute("attr")?.to_string()]).build()?])
        .build()?;
    tenant.registrations().register(&registration_8).send().await?;
    let _registration_8_cleanup = Cleanup::of(&tenant).registration(&registration_8_id);
    let page = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(format!("https://test.andromeda/{namespace}/T1").as_str())
        .ids(vec![vehicle_covered_id.clone()])
        .send()
        .await?;
    expect_that!(page.items(), contains(has_id(&registration_id)));
    let page_2 = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(format!("https://test.andromeda/{namespace}/T1").as_str())
        .ids(vec![vehicle_other_id.clone()])
        .send()
        .await?;
    expect_that!(page_2.items(), not(contains(has_id(&registration_id))));
    let page_3 = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(format!("https://test.andromeda/{namespace}/T1").as_str())
        .ids(vec![vehicle_covered_id.clone()])
        .attrs(vec!["brandName".parse()?])
        .send()
        .await?;
    expect_that!(page_3.items(), not(contains(has_id(&registration_id))));
    let page_4 = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(format!("https://test.andromeda/{namespace}/T2b").as_str())
        .send()
        .await?;
    expect_that!(page_4.items(), contains(has_id(&registration_2_id)));
    let page_5 = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(format!("https://test.andromeda/{namespace}/T2c").as_str())
        .send()
        .await?;
    expect_that!(page_5.items(), not(contains(has_id(&registration_2_id))));
    let page_6 = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(format!("https://test.andromeda/{namespace}/T1").as_str())
        .attrs(vec!["speed".parse()?])
        .send()
        .await?;
    expect_that!(page_6.items(), contains(has_id(&registration_id)));
    let page_7 = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(format!("https://test.andromeda/{namespace}/T1").as_str())
        .attrs(vec!["brandName".parse()?])
        .send()
        .await?;
    expect_that!(page_7.items(), not(contains(has_id(&registration_id))));
    let page_8 = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(format!("https://test.andromeda/{namespace}/T2a").as_str())
        .attrs(vec!["speed".parse()?])
        .send()
        .await?;
    expect_that!(page_8.items(), contains(has_id(&registration_2_id)));
    let page_9 = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(format!("https://test.andromeda/{namespace}/T3").as_str())
        .ids(vec![vehicle_covered_id.clone()])
        .send()
        .await?;
    expect_that!(page_9.items(), contains(has_id(&registration_3_id)));
    let page_10 = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(format!("https://test.andromeda/{namespace}/T3x").as_str())
        .ids(vec![vehicle_covered_id.clone()])
        .send()
        .await?;
    expect_that!(page_10.items(), not(contains(has_id(&registration_3_id))));
    let page_11 = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(format!("https://test.andromeda/{namespace}/T3").as_str())
        .ids(vec![vehicle_other_id.clone()])
        .send()
        .await?;
    expect_that!(page_11.items(), not(contains(has_id(&registration_3_id))));
    let page_12 = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(format!("https://test.andromeda/{namespace}/T4").as_str())
        .ids(vec![vehicle_anything_id])
        .send()
        .await?;
    expect_that!(page_12.items(), contains(has_id(&registration_4_id)));
    let page_13 = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(format!("https://test.andromeda/{namespace}/T5").as_str())
        .ids(vec![fleet_id])
        .send()
        .await?;
    expect_that!(page_13.items(), contains(has_id(&registration_5_id)));
    let page_14 = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(format!("https://test.andromeda/{namespace}/T5").as_str())
        .ids(vec![other_id])
        .send()
        .await?;
    expect_that!(page_14.items(), not(contains(has_id(&registration_5_id))));
    let page_15 = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(format!("https://test.andromeda/{namespace}/T6").as_str())
        .id_pattern(namespace.id_pattern("Fleet")?)
        .send()
        .await?;
    expect_that!(page_15.items(), contains(has_id(&registration_6_id)));
    let page_16 = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(format!("https://test.andromeda/{namespace}/T6").as_str())
        .id_pattern(namespace.id_pattern("Nowhere")?)
        .send()
        .await?;
    expect_that!(page_16.items(), not(contains(has_id(&registration_6_id))));
    let page_17 = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(format!("https://test.andromeda/{namespace}/T7").as_str())
        .id_pattern(namespace.id_pattern("Fleet")?)
        .send()
        .await?;
    expect_that!(page_17.items(), contains(has_id(&registration_7_id)));
    let page_18 = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .attrs(vec![format!("attr_{namespace}").parse()?])
        .send()
        .await?;
    expect_that!(page_18.items(), contains(has_id(&registration_8_id)));
    Ok(())
}
