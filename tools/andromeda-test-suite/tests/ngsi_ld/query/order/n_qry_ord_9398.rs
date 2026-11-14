//! A language property orders by its string.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, LanguageMap, LanguageProperty, NgsiLdMediaType},
    query::{Locality, ordering::OrderBy},
};

/// Pins: with local scope, orderBy=v over an Entity whose `v` is the `LanguageProperty` {"en": "a"} and an Entity
/// whose `v` is the Property "b" orders the `LanguageProperty` first, and orderBy=v;desc orders it last; ordering is
/// by target value, and the target value of a `LanguageProperty` addressed with no language is a String from its
/// map
/// Clause: 4.23.1, 4.23.2, 4.9, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_qry_ord_9398_a_language_property_orders_by_its_string() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let label_type = namespace.entity_type("Label")?;
    let language_id = namespace.urn("Label", "language")?;
    let plain_id = namespace.urn("Label", "plain")?;
    let entities = [
        Entity::builder()
            .id(language_id.clone())
            .entity_type(label_type.clone())
            .language_property("v", LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "a")])?))
            .build()?,
        Entity::builder()
            .id(plain_id.clone())
            .entity_type(label_type.clone())
            .property("v", "b")
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let ascending = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&label_type)
        .locality(Locality::Local)
        .ordering("v".parse::<OrderBy>()?)
        .send()
        .await?;
    expect_that!(ascending.items(), len(eq(2)));
    expect_that!(ascending.items(), nth(0, has_id(&language_id)));
    expect_that!(ascending.items(), nth(1, has_id(&plain_id)));
    let descending = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&label_type)
        .locality(Locality::Local)
        .ordering("v;desc".parse::<OrderBy>()?)
        .send()
        .await?;
    expect_that!(descending.items(), len(eq(2)));
    expect_that!(descending.items(), nth(0, has_id(&plain_id)));
    expect_that!(descending.items(), nth(1, has_id(&language_id)));
    Ok(())
}
