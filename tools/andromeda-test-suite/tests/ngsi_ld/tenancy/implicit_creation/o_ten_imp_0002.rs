//! An operation that creates no tenant is nonexistent tenant on an unknown one.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::{HeaderName, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, ContextEntry, Entity, EntityFragment, ErrorType, InlineContext, NgsiLdMediaType},
    registration::{CSourceRegistration, RegistrationInfo},
};

/// Pins: Batch Entity Upsert, Batch Entity Merge, Update Attributes, Append Attributes, Delete Entity, Batch Entity
/// Delete and Add @context on a Tenant that does not exist each answer 404 `NonexistentTenant` echoing the tenant,
/// and none of them creates it
/// Clause: 5.5.10, 6.3.14, 5.6.8, 5.6.20, 5.6.2, 5.6.3, 5.6.6, 5.6.10, 5.13.2
/// Tenant: private (the subject is Tenants that do not exist)
/// Recorded: clause 5.5.10 makes implicit Tenant creation an implementation option and says the other operations on
/// a non-existing Tenant "should" raise `NonexistentTenant`; Andromeda creates Tenants implicitly, exactly on the
/// six operations the clause lists
#[gtest]
#[tokio::test]
async fn o_ten_imp_0002_an_operation_that_creates_no_tenant_is_nonexistent_tenant_on_an_unknown_one() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let tenant = broker.with_tenant(namespace.tenant()?);
    let registration_provision_id = namespace.registration_id("provision")?;
    let provisioning_sentinel_type = namespace.entity_type("ProvisioningSentinel")?;
    let u1_tenant = broker.with_tenant(format!("{namespace}-u1").parse()?);
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let u2_tenant = broker.with_tenant(format!("{namespace}-u2").parse()?);
    let u3_tenant = broker.with_tenant(format!("{namespace}-u3").parse()?);
    let u4_tenant = broker.with_tenant(format!("{namespace}-u4").parse()?);
    let u5_tenant = broker.with_tenant(format!("{namespace}-u5").parse()?);
    let u6_tenant = broker.with_tenant(format!("{namespace}-u6").parse()?);
    let u7_tenant = broker.with_tenant(format!("{namespace}-u7").parse()?);
    let registration_provision = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_provision_id.clone())
        .information([RegistrationInfo::builder().entities([provisioning_sentinel_type.as_str()]).build()?])
        .build()?;
    tenant.registrations().register(&registration_provision).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_provision_cleanup = Cleanup::of(&tenant).registration(&registration_provision_id);
    tenant.registrations().delete(&registration_provision_id).send().await?;
    // Batch Upsert and Batch Merge create entities but are not among the operations clause 5.5.10 lists as creating a Tenant.
    let entities = [Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("name", "v1")
        .build()?];
    let answer = u1_tenant.with_responses().entities().batch_upsert(&entities).send().await;
    expect_that!(
        &answer,
        problem(StatusCode::NOT_FOUND, ErrorType::NonexistentTenant).with_header(HeaderName::from_static("ngsild-tenant"), &format!("{namespace}-u1"))
    );
    let answer_2 = u1_tenant
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .send()
        .await;
    expect_that!(
        &answer_2,
        problem(StatusCode::NOT_FOUND, ErrorType::NonexistentTenant).with_header(HeaderName::from_static("ngsild-tenant"), &format!("{namespace}-u1"))
    );
    let fragments = [EntityFragment::for_entity(vehicle_id.clone(), vehicle_type.clone())
        .property("name", "v1")
        .build()?];
    let answer_3 = u2_tenant.with_responses().entities().batch_merge(&fragments).send().await;
    expect_that!(
        &answer_3,
        problem(StatusCode::NOT_FOUND, ErrorType::NonexistentTenant).with_header(HeaderName::from_static("ngsild-tenant"), &format!("{namespace}-u2"))
    );
    let answer_4 = u2_tenant
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .send()
        .await;
    expect_that!(
        &answer_4,
        problem(StatusCode::NOT_FOUND, ErrorType::NonexistentTenant).with_header(HeaderName::from_static("ngsild-tenant"), &format!("{namespace}-u2"))
    );
    let fragment = EntityFragment::builder().property("name", "fragment").build()?;
    let answer_5 = u3_tenant.with_responses().entities().update_attributes(&vehicle_id, &fragment).send().await;
    expect_that!(
        &answer_5,
        problem(StatusCode::NOT_FOUND, ErrorType::NonexistentTenant).with_header(HeaderName::from_static("ngsild-tenant"), &format!("{namespace}-u3"))
    );
    let answer_6 = u3_tenant
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .send()
        .await;
    expect_that!(
        &answer_6,
        problem(StatusCode::NOT_FOUND, ErrorType::NonexistentTenant).with_header(HeaderName::from_static("ngsild-tenant"), &format!("{namespace}-u3"))
    );
    let fragment_2 = EntityFragment::builder().property("name", "fragment").build()?;
    let answer_7 = u4_tenant.with_responses().entities().append_attributes(&vehicle_id, &fragment_2).send().await;
    expect_that!(
        &answer_7,
        problem(StatusCode::NOT_FOUND, ErrorType::NonexistentTenant).with_header(HeaderName::from_static("ngsild-tenant"), &format!("{namespace}-u4"))
    );
    let answer_8 = u4_tenant
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .send()
        .await;
    expect_that!(
        &answer_8,
        problem(StatusCode::NOT_FOUND, ErrorType::NonexistentTenant).with_header(HeaderName::from_static("ngsild-tenant"), &format!("{namespace}-u4"))
    );
    let answer_9 = u5_tenant.with_responses().entities().delete(&vehicle_id).send().await;
    expect_that!(
        &answer_9,
        problem(StatusCode::NOT_FOUND, ErrorType::NonexistentTenant).with_header(HeaderName::from_static("ngsild-tenant"), &format!("{namespace}-u5"))
    );
    let answer_10 = u5_tenant
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .send()
        .await;
    expect_that!(
        &answer_10,
        problem(StatusCode::NOT_FOUND, ErrorType::NonexistentTenant).with_header(HeaderName::from_static("ngsild-tenant"), &format!("{namespace}-u5"))
    );
    let ids = [vehicle_id.clone()];
    let answer_11 = u6_tenant.with_responses().entities().batch_delete(&ids).send().await;
    expect_that!(
        &answer_11,
        problem(StatusCode::NOT_FOUND, ErrorType::NonexistentTenant).with_header(HeaderName::from_static("ngsild-tenant"), &format!("{namespace}-u6"))
    );
    let answer_12 = u6_tenant
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .send()
        .await;
    expect_that!(
        &answer_12,
        problem(StatusCode::NOT_FOUND, ErrorType::NonexistentTenant).with_header(HeaderName::from_static("ngsild-tenant"), &format!("{namespace}-u6"))
    );
    let context = Context::Single(ContextEntry::Inline(
        format!(r#"{{"noncreating": "https://test.andromeda/{namespace}/noncreating"}}"#).parse::<InlineContext>()?,
    ));
    let answer_13 = u7_tenant.with_responses().contexts().add(&context).send().await;
    expect_that!(
        &answer_13,
        problem(StatusCode::NOT_FOUND, ErrorType::NonexistentTenant).with_header(HeaderName::from_static("ngsild-tenant"), &format!("{namespace}-u7"))
    );
    let answer_14 = u7_tenant
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .send()
        .await;
    expect_that!(
        &answer_14,
        problem(StatusCode::NOT_FOUND, ErrorType::NonexistentTenant).with_header(HeaderName::from_static("ngsild-tenant"), &format!("{namespace}-u7"))
    );
    Ok(())
}
