//! A csf with an unquoted uri containing dots is a valid filter.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::RegisterContextSource},
    model::{NgsiLdMediaType, RawJson, Uri},
    query::q,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Query Context Source Registrations accepts a csf whose operand is an unquoted URI containing dots, and
/// selects the registration whose property equals it
/// Clause: 4.9, 5.10.2.4, 6.8.3.2
#[gtest]
#[tokio::test]
async fn n_csr_qry_3060_a_csf_with_an_unquoted_uri_containing_dots_is_a_valid_filter() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let thing_type = namespace.entity_type("Thing")?;
    let registration = CSourceRegistration::builder(source.endpoint("1")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(thing_type.clone()).with_id_pattern(namespace.id_pattern("Thing")?)])
            .build()?])
        .build()?;
    // The model refuses it (a registration a registration property), so `site` is written as text.
    let site: RawJson = r#"{"type": "Property", "value": "http://example.org/a"}"#.parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("site", &site);
    let request = broker.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    // CompEqualityValue = OtherValue / ValueList / Range / URI: the unquoted URI carries dots
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(&thing_type)
        .csf(q::attr("site").eq(q::UnquotedUri::try_from("http://example.org/a".parse::<Uri>()?)?))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&registration_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(&thing_type)
        .csf(q::attr("site").ne(q::UnquotedUri::try_from("http://example.org/b".parse::<Uri>()?)?))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&registration_id)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(&thing_type)
        .csf(q::attr("site").eq(q::UnquotedUri::try_from("http://example.org/b".parse::<Uri>()?)?))
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(0)));
    Ok(())
}
