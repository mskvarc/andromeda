//! A write is unaffected by accept language.

use andromeda_test_suite::{attribute_matcher::attribute_of, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done, wire::vary};
use googletest::prelude::*;
use http::{HeaderName, HeaderValue};
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeKind, Entity, EntityFragment, LanguageMap, LanguageProperty, NgsiLdMediaType},
};

/// Pins: Create Entity and Update Attributes carrying Accept-Language `en_US` succeed, because only the operations
/// that take lang read the header, and their responses do not vary on accept-language
/// Clause: 5.6.1, 5.6.2
/// Recorded: Andromeda extension, not v1.9.1: the `accept_language` extension reads Accept-Language as the clause
/// 4.15 language filter where no lang names one; NGSI-LD carries the filter in lang alone
#[gtest]
#[tokio::test]
async fn a_ext_lng_0011_a_write_is_unaffected_by_accept_language() -> Result<()> {
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
    let received = broker
        .with_responses()
        .with_header_field(HeaderName::from_static("accept-language"), HeaderValue::from_static("en_US"))?
        .entities()
        .create(&building)
        .send()
        .await?;
    received.outcome.done()?;
    expect_that!(vary(received.head.headers()), not(contains(eq(&HeaderName::from_static("accept-language")))));
    let fragment = EntityFragment::builder()
        .language_property(
            "street",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("fr", "Place Royale"), ("nl", "Koningsplein")])?),
        )
        .build()?;
    broker
        .with_header_field(HeaderName::from_static("accept-language"), HeaderValue::from_static("en_US"))?
        .entities()
        .update_attributes(&building_id, &fragment)
        .send()
        .await?
        .done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute(
            "street",
            attribute_of(AttributeKind::LanguageProperty)
                .language("fr", "Place Royale")
                .language("nl", "Koningsplein")
        )
    );
    Ok(())
}
