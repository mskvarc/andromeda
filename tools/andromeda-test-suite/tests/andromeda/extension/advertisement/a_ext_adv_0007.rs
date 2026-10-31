//! Conformance mode ignores accept language.

use andromeda_test_suite::{
    attribute_matcher::attribute_of,
    attribute_member::Member,
    broker,
    config::BrokerRole,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    wire::vary,
};
use googletest::prelude::*;
use http::HeaderName;
use ngsi_ld::{
    http::headers::{Accept, AcceptLanguage},
    model::{AttributeKind, Entity, LanguageMap, LanguageProperty, NgsiLdMediaType},
    query::language_filter::LanguageFilter,
};

/// Pins: a broker in conformance mode, with the accept-language flag also on, answers Retrieve Entity carrying
/// Accept-Language nl with the whole `languageMap` and does not vary the response on accept-language, as a broker
/// without the extension does
/// Clause: 4.15, 4.5.18, 5.7.1.4
/// Recorded: Andromeda extension, not v1.9.1: conformance mode forces every extension off whatever its own flag
/// says, accept-language included
#[gtest]
#[tokio::test]
async fn a_ext_adv_0007_conformance_mode_ignores_accept_language() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_conformance = broker::client(BrokerRole::Conformance)?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .language_property(
            "street",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("fr", "Grand Place"), ("nl", "Grote Markt")])?),
        )
        .build()?;
    broker_conformance.entities().create(&building).send().await?.done()?;
    let headed = broker_conformance
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .with_header(AcceptLanguage::try_from("nl".parse::<LanguageFilter>()?)?)?
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    expect_that!(vary(headed.head.headers()), not(contains(eq(&HeaderName::from_static("accept-language")))));
    let retrieved = &headed.outcome;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute(
            "street",
            attribute_of(AttributeKind::LanguageProperty)
                .language("fr", "Grand Place")
                .language("nl", "Grote Markt")
                .without(Member::Lang)
        )
    );
    Ok(())
}
