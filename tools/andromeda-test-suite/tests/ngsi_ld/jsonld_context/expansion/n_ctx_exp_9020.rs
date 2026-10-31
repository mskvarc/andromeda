//! A json property keeps a context key as data.

use andromeda_test_suite::{
    attribute_matcher::attribute_of,
    broker,
    core_context::core_context_list,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{AttributeKind, Entity, JsonProperty, NgsiLdMediaType, RawJson},
};
use serde_json::json;

/// Pins: Create Entity with a `JsonProperty` whose json holds @context keys at its top, in a nested object and in
/// an array item answers 201, and the json reads back unchanged
/// Clause: 5.5.7, 4.5.24, 5.6.1.4
#[gtest]
#[tokio::test]
async fn n_ctx_exp_9020_a_json_property_keeps_a_context_key_as_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let body_context = broker.with_context(core_context_list()?)?.with_context_delivery(ContextDelivery::LdJsonBody);
    let data: RawJson = format!(
        r#"{{"@context": "https://example.org/{namespace}/this-is-data-not-a-context", "nested": {{"@context": {{"shipped": "as-is"}}, "count": 3}}, "list": [{{"@context": []}}, 7]}}"#
    )
    .parse()?;
    let device = Entity::builder()
        .id(device_id.clone())
        .entity_type(device_type)
        .json_property("config", JsonProperty::builder(data))
        .build()?;
    body_context.entities().create(&device).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    expect_that!(
        retrieved.body(),
        has_attribute(
            "config",
            attribute_of(AttributeKind::JsonProperty).json(json!({
                "@context": format!("https://example.org/{namespace}/this-is-data-not-a-context"),
                "nested": {"@context": {"shipped": "as-is"}, "count": 3},
                "list": [{"@context": []}, 7]
            }))
        )
    );
    Ok(())
}
