//! A request naming no language keeps the whole language map.

use andromeda_test_suite::{
    attribute_matcher::attribute_of,
    attribute_member::Member,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    wire::{request_to, vary},
};
use googletest::prelude::*;
use http::{
    HeaderName,
    HeaderValue,
    Method,
    StatusCode,
    header::{ACCEPT, ACCEPT_LANGUAGE},
};
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeKind, Entity, LanguageMap, LanguageProperty, Lossless, NgsiLdMediaType, Output},
};
use std::str;

/// Pins: Retrieve Entity without Accept-Language, and with an Accept-Language whose list names no language, answers
/// the whole `languageMap`, and the response still varies on accept-language
/// Clause: 4.15, 4.5.18, 5.7.1.4
/// Recorded: Andromeda extension, not v1.9.1: the `accept_language` extension reads Accept-Language as the clause
/// 4.15 language filter where no lang names one; RFC 9110 section 12.5.5 has a response that depends on the header
/// vary on it whether or not the request carried it, and section 5.6.1 skips empty list elements
#[gtest]
#[tokio::test]
async fn a_ext_lng_0010_a_request_naming_no_language_keeps_the_whole_language_map() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
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
    broker.entities().create(&building).send().await?.done()?;
    let whole_map = || {
        has_attribute(
            "street",
            attribute_of(AttributeKind::LanguageProperty)
                .language("fr", "Grand Place")
                .language("nl", "Grote Markt")
                .without(Member::Lang),
        )
    };
    let headed = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    expect_that!(vary(headed.head.headers()), contains(eq(&HeaderName::from_static("accept-language"))));
    let retrieved = &headed.outcome;
    expect_that!(retrieved.body(), whole_map());
    // A list of empty elements names no language, which the typed header cannot write.
    let request = request_to(&broker, Method::GET, &format!("entities/{building_id}"))?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"))
        .append_header(ACCEPT_LANGUAGE, HeaderValue::from_static(","));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::OK));
    let entity = Entity::<Output>::parse(str::from_utf8(answer?.body())?, Lossless::Normalized)?;
    expect_that!(&entity, whole_map());
    Ok(())
}
