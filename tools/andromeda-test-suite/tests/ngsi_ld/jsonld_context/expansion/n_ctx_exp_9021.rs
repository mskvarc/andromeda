//! A top level context is admitted.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    core_context::{core_context_list, core_entry},
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{
        Context,
        ContextEntry,
        Entity,
        EntityFragment,
        InlineContext,
        LanguageMap,
        LanguageProperty,
        NgsiLdMediaType,
        Property,
        Value,
        geometry::{Position, Shape},
    },
};

/// Pins: Create Entity whose top-level @context adds a term to the Core @context, with a sub-Attribute, a
/// `GeoProperty` and a `LanguageProperty`, answers 201, and Append Attributes under the Core @context answers 204
/// and is stored
/// Clause: 5.5.7, 5.6.1.4, 5.6.3.4
#[gtest]
#[tokio::test]
async fn n_ctx_exp_9021_a_top_level_context_is_admitted() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let body_context = broker
        .with_context(Context::Multiple(vec![
            core_entry()?,
            ContextEntry::Inline(format!(r#"{{"speed": "https://example.org/{namespace}/speed"}}"#).parse::<InlineContext>()?),
        ]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let core_context_list_body = broker.with_context(core_context_list()?)?.with_context_delivery(ContextDelivery::LdJsonBody);
    let device = Entity::builder()
        .id(device_id.clone())
        .entity_type(device_type)
        .property("speed", Property::builder(1).property("accuracy", Value::Number("0.5".parse()?)))
        .geo_property("location", Shape::Point(Position::try_new(13.4, 52.5)?))
        .language_property("label", LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "hello")])?))
        .build()?;
    body_context.entities().create(&device).send().await?.done()?;
    let fragment = EntityFragment::builder().property("gear", 3).build()?;
    core_context_list_body
        .entities()
        .append_attributes(&device_id, &fragment)
        .send()
        .await?
        .done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("gear", any_attribute().value(3)));
    Ok(())
}
