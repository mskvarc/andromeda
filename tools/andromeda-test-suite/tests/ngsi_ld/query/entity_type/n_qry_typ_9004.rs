//! Several selectors keep each type with its pattern.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, identity_matcher::has_id, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{QueryMethod, ResultsCount},
    },
    model::{Entity, Input, NgsiLdMediaType},
    query::{EntitySelector, IdPattern, builders::SelectorTypeBuilder},
    registration::{CSourceRegistration, RegistrationInfo},
};
use std::iter;

/// Pins: Query Entities through POST with one selector of the early type and the id pattern of its first Entity and
/// one of the late type answers that Entity and the six of the late type, counted 7
/// Clause: 5.2.33, 4.17, 6.3.13
/// Tenant: private (a query naming several `EntitySelectors` is forwarded to every registration of the tenant, and the
/// Entities those sources answer join its answer, so in the shared tenant the registrations of other files would fill
/// it)
#[gtest]
#[tokio::test]
async fn n_qry_typ_9004_several_selectors_keep_each_type_with_its_pattern() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let tenant = broker.with_tenant(namespace.tenant()?);
    let registration_provision_id = namespace.registration_id("provision")?;
    let sentinel_type = namespace.entity_type("Sentinel")?;
    let a_0_id = namespace.urn("A", "0")?;
    let membership_early_type = namespace.entity_type("MembershipEarly")?;
    let m_0_id = namespace.urn("M", "0")?;
    let z_0_id = namespace.urn("Z", "0")?;
    let membership_late_type = namespace.entity_type("MembershipLate")?;
    let z_id = namespace.urn("Z", 1)?;
    let z_2_id = namespace.urn("Z", 2)?;
    let z_3_id = namespace.urn("Z", 3)?;
    let z_4_id = namespace.urn("Z", 4)?;
    let registration_provision = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_provision_id.clone())
        .information([RegistrationInfo::builder().entities([sentinel_type.as_str()]).build()?])
        .build()?;
    tenant.registrations().register(&registration_provision).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_provision_cleanup = Cleanup::of(&tenant).registration(&registration_provision_id);
    tenant.registrations().delete(&registration_provision_id).send().await?;
    let entities = (0..=5)
        .map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("A", n)?)
                .entity_type(membership_early_type.clone())
                .property("name", format!("urn:ngsi-ld:A:{namespace}-{n}"))
                .build()?)
        })
        .chain(iter::once(Ok(Entity::builder()
            .id(m_0_id.clone())
            .entity_type(vec![membership_early_type.clone(), namespace.entity_type("MembershipLate")?])
            .property("name", format!("urn:ngsi-ld:M:{namespace}-0"))
            .build()?)))
        .chain((0..=4).map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Z", n)?)
                .entity_type(membership_late_type.clone())
                .property("name", format!("urn:ngsi-ld:Z:{namespace}-{n}"))
                .build()?)
        }))
        .collect::<Result<Vec<_>>>()?;
    tenant.entities().batch_create(&entities).send().await?.done()?;
    let selectors = [
        EntitySelector::new(SelectorTypeBuilder::from(&membership_early_type).build()?)
            .with_id_pattern(format!("^urn:ngsi-ld:A:{namespace}-0$").parse::<IdPattern>()?),
        EntitySelector::new(SelectorTypeBuilder::from(&membership_late_type).build()?)
            .with_id_pattern(format!("urn:ngsi-ld:.+:{namespace}-.*").parse::<IdPattern>()?),
    ];
    let page = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .selectors(selectors)
        .count(ResultsCount::Include)
        .limit(100)
        .send()
        .await?;
    expect_that!(page.total(), some(eq(7)));
    expect_that!(page.items(), len(eq(7)));
    expect_that!(page.items(), contains(has_id(&a_0_id)));
    expect_that!(page.items(), contains(has_id(&m_0_id)));
    expect_that!(page.items(), contains(has_id(&z_0_id)));
    expect_that!(page.items(), contains(has_id(&z_id)));
    expect_that!(page.items(), contains(has_id(&z_2_id)));
    expect_that!(page.items(), contains(has_id(&z_3_id)));
    expect_that!(page.items(), contains(has_id(&z_4_id)));
    Ok(())
}
