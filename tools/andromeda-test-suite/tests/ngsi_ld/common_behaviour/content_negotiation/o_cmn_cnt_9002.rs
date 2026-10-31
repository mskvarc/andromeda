//! A registration and each item of its collection open with the context.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    query::IdPattern,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Retrieve Context Source Registration answered as application/ld+json opens with @context, and so does the
/// item of each page of Query Context Source Registrations by type
/// Clause: 6.3.6, 5.10.1, 5.10.2
/// Recorded: 6.3.6 requires the @context in an application/ld+json body and fixes no member order (RFC 8259 objects
/// are unordered); Andromeda writes it as the first member, of every item of a collection too
#[gtest]
#[tokio::test]
async fn o_cmn_cnt_9002_a_registration_and_each_item_of_its_collection_open_with_the_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_2_id = namespace.registration_id(2)?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(format!("urn:ngsi-ld:.+:{namespace}-.*").parse::<IdPattern>()?)])
            .build()?])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let registration_2 = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(format!("urn:ngsi-ld:.+:{namespace}-.*").parse::<IdPattern>()?)])
            .build()?])
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    let ld_json = broker.with_responses().with_accept(Accept::new(NgsiLdMediaType::JsonLd));
    let retrieved = ld_json.registrations().retrieve(&registration_id).send().await?;
    expect_that!(&String::from_utf8_lossy(&retrieved.body).into_owned(), contains_regex(r#"^\s*\{\s*"@context""#));
    // One item per page: the collection is the tenant's, and each page shows how one item opens.
    let page = ld_json.registrations().query().entity_type(&vehicle_type).limit(1).offset(0).send().await?;
    expect_that!(page.outcome.items(), len(eq(1)));
    expect_that!(&String::from_utf8_lossy(&page.body).into_owned(), contains_regex(r#"^\s*\[\s*\{\s*"@context""#));
    let page_2 = ld_json.registrations().query().entity_type(&vehicle_type).limit(1).offset(1).send().await?;
    expect_that!(page_2.outcome.items(), len(eq(1)));
    expect_that!(
        &String::from_utf8_lossy(&page_2.body).into_owned(),
        contains_regex(r#"^\s*\[\s*\{\s*"@context""#)
    );
    broker.registrations().delete(&registration_id).send().await?;
    broker.registrations().delete(&registration_2_id).send().await?;
    Ok(())
}
