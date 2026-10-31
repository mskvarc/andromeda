//! An entity id with a type matches the registration naming it and the type wide one.

use andromeda_test_suite::{
    broker,
    hosted_context::HostedContext,
    identity_matcher::has_id,
    namespace::Namespace,
    registration_vocabulary::registration_vocabulary,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, Input, NgsiLdMediaType, OneOrMany},
    registration::{CSourceRegistration, EntityInfo, RegistrationId, RegistrationInfo},
};

/// Pins: Query Context Source Registrations by type and one entity id answers 200 with the registration naming that
/// entity and the registration naming the type alone, and not the registration naming another entity
/// Clause: 5.10.2, 5.10.2.3, 5.12
/// Tenant: private (the subject is a registration naming the type alone, which clause 5.12 matches against every
/// id-only request of its tenant)
#[gtest]
#[tokio::test]
async fn n_csr_qry_0011_an_entity_id_with_a_type_matches_the_registration_naming_it_and_the_type_wide_one() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let ctx = HostedContext::add(&tenant, &registration_vocabulary(&namespace)?).await?;
    let body_context = tenant
        .with_context(Context::Multiple(vec![ctx.entry()?]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_a_id = namespace.urn("Vehicle", "A456")?;
    let vehicle_b_id = namespace.urn("Vehicle", "B789")?;
    let registration_1_id = namespace.registration_id(1)?;
    let registration_2_id = namespace.registration_id(2)?;
    let registration_3_id = namespace.registration_id(3)?;
    let narrowed = |id: &RegistrationId, entity: EntityInfo| -> Result<CSourceRegistration<Input>> {
        Ok(CSourceRegistration::builder("http://my.csource.org:1026/")
            .id(id.clone())
            .information([RegistrationInfo::builder().entities([entity]).property_names(["speed"]).build()?])
            .build()?)
    };
    let registration_1 = narrowed(&registration_1_id, EntityInfo::new(vehicle_type.clone()).with_id(vehicle_a_id.clone()))?;
    body_context.registrations().register(&registration_1).send().await?;
    let registration_2 = narrowed(&registration_2_id, EntityInfo::new(vehicle_type.clone()).with_id(vehicle_b_id))?;
    body_context.registrations().register(&registration_2).send().await?;
    let registration_3 = narrowed(&registration_3_id, EntityInfo::new(vehicle_type.clone()))?;
    body_context.registrations().register(&registration_3).send().await?;
    let linked = tenant.with_context(Context::Single(ctx.entry()?))?;
    let page = linked
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .registrations()
        .query()
        .entity_type(&vehicle_type)
        .ids(vec![vehicle_a_id.clone()])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), contains(has_id(&registration_1_id)));
    expect_that!(page.items(), contains(has_id(&registration_3_id)));
    let first = page.items().iter().find(|registration| registration.id() == &registration_1_id).or_fail()?;
    expect_that!(
        first.information().first().and_then(|info| info.entities().first()).and_then(EntityInfo::id),
        some(eq(&vehicle_a_id))
    );
    let third = page.items().iter().find(|registration| registration.id() == &registration_3_id).or_fail()?;
    expect_that!(
        third.information().first().and_then(|info| info.entities().first()).and_then(EntityInfo::id),
        none()
    );
    expect_that!(
        third
            .information()
            .first()
            .and_then(|info| info.entities().first())
            .map(EntityInfo::entity_type),
        some(eq(&OneOrMany::one(vehicle_type.clone())))
    );
    Ok(())
}
