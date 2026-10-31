//! Attrs naming a full iri selects the registration under a user context.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    hosted_context::HostedContext,
    identity_matcher::has_id,
    namespace::Namespace,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, ContextEntry, InlineContext, NgsiLdMediaType},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Query Context Source Registrations with attrs naming an attribute by its full IRI or a compact IRI selects
/// the registration that registers that attribute and not one that registers another, under a Link header carrying
/// a user context, as the short name does
/// Clause: 5.5.7, 5.10.2.4, 6.8.3.2
#[gtest]
#[tokio::test]
async fn n_csr_qry_3080_attrs_naming_a_full_iri_selects_the_registration_under_a_user_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let registration_2_id = namespace.registration_id(2)?;
    let building_id = namespace.urn("Building", 1)?;
    let context = Context::Single(ContextEntry::Inline(format!(r#"{{"ex": "https://test.andromeda/{namespace}/", "Building_{namespace}": "https://test.andromeda/{namespace}/Building", "subCategory": "https://test.andromeda/{namespace}/subCategory", "airQualityLevel": "https://test.andromeda/{namespace}/airQualityLevel"}}"#).parse::<InlineContext>()?));
    let ctx = HostedContext::add(&broker, &context).await?;
    let linked = broker.with_context(Context::Single(ctx.entry()?))?;
    let registration = CSourceRegistration::builder(source.endpoint("1")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern("Building")?)])
            .property_names(["subCategory"])
            .build()?])
        .build()?;
    linked.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&linked).registration(&registration_id);
    let registration_2 = CSourceRegistration::builder(source.endpoint("1")?)
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern("Building")?)])
            .property_names(["airQualityLevel"])
            .build()?])
        .build()?;
    linked.registrations().register(&registration_2).send().await?;
    let _registration_2_cleanup = Cleanup::of(&linked).registration(&registration_2_id);
    // the id keeps the answer to this file's registrations; the attribute list is the narrowing input. 5.5.7: an absolute IRI expands to itself
    let page = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .attrs(vec!["subCategory".parse()?])
        .ids(vec![building_id.clone()])
        .send()
        .await?;
    expect_that!(page.items(), contains(has_id(&registration_id)));
    expect_that!(page.items(), not(contains(has_id(&registration_2_id))));
    let page_2 = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .attrs(vec![format!("https://test.andromeda/{namespace}/subCategory").parse()?])
        .ids(vec![building_id.clone()])
        .send()
        .await?;
    expect_that!(page_2.items(), contains(has_id(&registration_id)));
    expect_that!(page_2.items(), not(contains(has_id(&registration_2_id))));
    let page_3 = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .attrs(vec!["ex:subCategory".parse()?])
        .ids(vec![building_id.clone()])
        .send()
        .await?;
    expect_that!(page_3.items(), contains(has_id(&registration_id)));
    expect_that!(page_3.items(), not(contains(has_id(&registration_2_id))));
    let page_4 = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .attrs(vec![format!("https://test.andromeda/{namespace}/airQualityLevel").parse()?])
        .ids(vec![building_id.clone()])
        .send()
        .await?;
    expect_that!(page_4.items(), contains(has_id(&registration_2_id)));
    expect_that!(page_4.items(), not(contains(has_id(&registration_id))));
    Ok(())
}
