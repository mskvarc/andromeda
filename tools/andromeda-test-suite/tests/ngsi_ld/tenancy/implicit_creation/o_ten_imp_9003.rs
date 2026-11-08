//! A create entity under an unreachable context creates no tenant.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::{HeaderName, StatusCode};
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, ContextEntry, Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: Create Entity on a new Tenant under a Link naming an @context nothing serves answers 504
/// `LdContextNotAvailable` and leaves the Tenant nonexistent: a following query there answers 404
/// `NonexistentTenant` echoing the tenant
/// Clause: 5.5.10, 5.6.1, 5.5.6, 6.3.14, Table 6.3.2-1
/// Tenant: private (the subject is creating a Tenant)
/// Recorded: clause 5.5.10 makes implicit Tenant creation an implementation option, created "when an NGSI-LD
/// operation for creating information targets a new Tenant"; Andromeda creates a Tenant only once a creating
/// operation has created information in it, so a refused one creates none
#[gtest]
#[tokio::test]
async fn o_ten_imp_9003_a_create_entity_under_an_unreachable_context_creates_no_tenant() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    broker::private_tenant(&broker, &namespace).await?;
    let refused_tenant = broker.with_tenant(format!("{namespace}-refused").parse()?);
    let refused_tenant_linked = refused_tenant.with_context(Context::Single(ContextEntry::Reference(IriRefBuf::new(format!(
        "{unreachable}/unreachable.jsonld"
    ))?)))?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id)
        .entity_type(vehicle_type.clone())
        .property("speed", 55)
        .build()?;
    let answer = refused_tenant_linked.with_responses().entities().create(&vehicle).send().await;
    expect_that!(&answer, problem(StatusCode::GATEWAY_TIMEOUT, ErrorType::LdContextNotAvailable));
    let answer_2 = refused_tenant
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .send()
        .await;
    expect_that!(
        &answer_2,
        problem(StatusCode::NOT_FOUND, ErrorType::NonexistentTenant).with_header(HeaderName::from_static("ngsild-tenant"), &format!("{namespace}-refused"))
    );
    Ok(())
}
