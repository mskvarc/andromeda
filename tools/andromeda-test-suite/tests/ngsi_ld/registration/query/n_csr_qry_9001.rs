//! A hop through one tenant does not exclude another tenants registration.

use andromeda_test_suite::{broker, config::SuiteConfig, identity_matcher::has_id, namespace::Namespace, nth::nth, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        UncheckedDocument,
        headers::{Accept, Via, ViaEntry},
        operation::RegisterContextSource,
    },
    model::{NgsiLdMediaType, RawJson},
    query::type_selection,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Query Context Source Registrations answers a registration naming the alias of a named tenant of this
/// broker with no Via and with a Via naming the default tenant's alias, and not with a Via naming that tenant's
/// alias
/// Clause: 4.3.6.4, 5.12, 4.14, Table 5.2.40-1
/// Tenant: private (the subject is how a named tenant identifies itself; the files name two tenants of their own,
/// which a registration created and deleted brings into existence)
#[gtest]
#[tokio::test]
async fn n_csr_qry_9001_a_hop_through_one_tenant_does_not_exclude_another_tenants_registration() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let alpha = broker::private_tenant(&broker, &namespace).await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let default_alias = broker.with_accept(Accept::any()).source_identity().send().await?.alias;
    let alpha_alias = alpha.with_accept(Accept::any()).source_identity().send().await?.alias;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern_ending(".+", ".*")?)])
            .build()?])
        .build()?;
    let alias: RawJson = format!(r#""{alpha_alias}""#).parse()?;
    // The model has no alias on a registration it writes, so `contextSourceAlias` is written as text.
    let document = UncheckedDocument::try_from(registration.render())?.set_member("contextSourceAlias", &alias);
    let request = broker.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let selection = type_selection::entity_type(&vehicle_type).build()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(selection.clone())
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&registration_id)));
    let via_default = broker.with_header(Via::try_from_entries([format!("1.1 {default_alias}").parse::<ViaEntry>()?])?)?;
    let page_2 = via_default
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(selection.clone())
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&registration_id)));
    let via_alpha = broker.with_header(Via::try_from_entries([format!("1.1 {alpha_alias}").parse::<ViaEntry>()?])?)?;
    let page_3 = via_alpha
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(selection)
        .send()
        .await?;
    expect_that!(page_3.items(), is_empty());
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
