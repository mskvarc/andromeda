//! A merge deletes the key a json literal sets to the null.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    core_context::core_entry,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    value::object,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        ContextDelivery,
        UncheckedDocument,
        headers::Accept,
        operation::{BatchMerge, MergeEntity},
    },
    model::{Attribute, Context, ContextEntry, Entity, EntityFragment, InlineContext, Lossless, NgsiLdMediaType, Property, RawJson, Value, ValuePatch},
};

/// Pins: Merge Entity and Batch Entity Merge of a JSON literal, in either spelling, holding urn:ngsi-ld:null under
/// a key answer 204 and delete that key from the held literal
/// Clause: 5.5.4, 5.5.12, 5.6.17, 5.6.20
#[gtest]
#[tokio::test]
async fn n_ent_mrg_9002_a_merge_deletes_the_key_a_json_literal_sets_to_the_null() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let body_context = broker
        .with_context(Context::Multiple(vec![
            ContextEntry::Inline(r#"{"settings": {"@id": "https://example.org/settings", "@type": "@json"}}"#.parse::<InlineContext>()?),
            core_entry()?,
        ]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let literal_probe_merge_keyed_id = namespace.urn("LiteralProbe", "merge-keyed")?;
    let literal_probe_type = namespace.entity_type("LiteralProbe")?;
    let literal_probe_batch_merge_keyed_id = namespace.urn("LiteralProbe", "batch-merge-keyed")?;
    let literal_probe_merge_typed_id = namespace.urn("LiteralProbe", "merge-typed")?;
    let literal_probe_batch_merge_typed_id = namespace.urn("LiteralProbe", "batch-merge-typed")?;
    let literal_probe_merge_keyed = Entity::builder()
        .id(literal_probe_merge_keyed_id.clone())
        .entity_type(literal_probe_type.clone())
        .property("config", object([("settings", object([("a", Value::from(1)), ("b", Value::from(2))]))]))
        .build()?;
    body_context.entities().create(&literal_probe_merge_keyed).send().await?.done()?;
    let fragment = EntityFragment::builder()
        .patch(
            "config",
            Attribute::from(Property::merge_builder(ValuePatch::new().merge("settings", ValuePatch::new().set("a", 1).delete("b"))).build()?),
        )
        .build()?;
    let answer = body_context
        .with_responses()
        .entities()
        .merge(&literal_probe_merge_keyed_id, &fragment)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&literal_probe_merge_keyed_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute(
            "config",
            any_attribute()
                .value_member(["https://example.org/settings", "@value", "a"], 1)
                .without_value_member(["https://example.org/settings", "@value", "b"])
        )
    );
    let literal_probe_batch_merge_keyed = Entity::builder()
        .id(literal_probe_batch_merge_keyed_id.clone())
        .entity_type(literal_probe_type.clone())
        .property("config", object([("settings", object([("a", Value::from(1)), ("b", Value::from(2))]))]))
        .build()?;
    body_context.entities().create(&literal_probe_batch_merge_keyed).send().await?.done()?;
    let literal_probe_batch_merge_keyed_fragment = EntityFragment::for_entity(literal_probe_batch_merge_keyed_id.clone(), literal_probe_type.clone())
        .patch(
            "config",
            Attribute::from(Property::merge_builder(ValuePatch::new().merge("settings", ValuePatch::new().set("a", 1).delete("b"))).build()?),
        )
        .build()?;
    let fragments = [literal_probe_batch_merge_keyed_fragment];
    let answer_2 = body_context.with_responses().entities().batch_merge(&fragments).send().await;
    expect_that!(&answer_2, answered(StatusCode::NO_CONTENT));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&literal_probe_batch_merge_keyed_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(
        entity_2,
        has_attribute(
            "config",
            any_attribute()
                .value_member(["https://example.org/settings", "@value", "a"], 1)
                .without_value_member(["https://example.org/settings", "@value", "b"])
        )
    );
    let literal_probe_merge_typed = Entity::builder()
        .id(literal_probe_merge_typed_id.clone())
        .entity_type(literal_probe_type.clone())
        .property(
            "config",
            object([
                ("@type", Value::from("@json")),
                ("@value", object([("a", Value::from(1)), ("b", Value::from(2))])),
            ]),
        )
        .build()?;
    body_context.entities().create(&literal_probe_merge_typed).send().await?.done()?;
    let fragment_2 = EntityFragment::builder().build()?;
    // The model refuses it (an attribute patch deleting members: {"type":"Property","value":{"@type":"@json","@value":{"a":1,"b":"urn:ngsi-ld:null"}}}), so `config` is written as text.
    let config_4: RawJson = r#"{"type": "Property", "value": {"@type": "@json", "@value": {"a": 1, "b": "urn:ngsi-ld:null"}}}"#.parse()?;
    let document_3 = UncheckedDocument::try_from(fragment_2.render(Lossless::Normalized))?.set_member("config", &config_4);
    let request_3 = body_context
        .unchecked(&MergeEntity::builder().id(&literal_probe_merge_typed_id).fragment(&fragment_2).build())?
        .document(&document_3);
    let answer_3 = body_context.send_unchecked(request_3).await;
    expect_that!(&answer_3, answered(StatusCode::NO_CONTENT));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&literal_probe_merge_typed_id)
        .send()
        .await?;
    let entity_3 = retrieved_3.body();
    expect_that!(
        entity_3,
        has_attribute("config", any_attribute().value_member(["@value", "a"], 1).without_value_member(["@value", "b"]))
    );
    let literal_probe_batch_merge_typed = Entity::builder()
        .id(literal_probe_batch_merge_typed_id.clone())
        .entity_type(literal_probe_type.clone())
        .property(
            "config",
            object([
                ("@type", Value::from("@json")),
                ("@value", object([("a", Value::from(1)), ("b", Value::from(2))])),
            ]),
        )
        .build()?;
    body_context.entities().create(&literal_probe_batch_merge_typed).send().await?.done()?;
    let literal_probe_batch_merge_typed_fragment =
        EntityFragment::for_entity(literal_probe_batch_merge_typed_id.clone(), literal_probe_type.clone()).build()?;
    // The model refuses it (an attribute patch deleting members: {"type":"Property","value":{"@type":"@json","@value":{"a":1,"b":"urn:ngsi-ld:null"}}}), so `config` is written as text.
    let config_5: RawJson = r#"{"type": "Property", "value": {"@type": "@json", "@value": {"a": 1, "b": "urn:ngsi-ld:null"}}}"#.parse()?;
    let document_4 = UncheckedDocument::try_from(literal_probe_batch_merge_typed_fragment.render(Lossless::Normalized))?.set_member("config", &config_5);
    let fragments_2 = [literal_probe_batch_merge_typed_fragment];
    let request_4 = body_context
        .unchecked(&BatchMerge::builder().fragments(&fragments_2).build())?
        .documents(&[document_4]);
    let answer_4 = body_context.send_unchecked(request_4).await;
    expect_that!(&answer_4, answered(StatusCode::NO_CONTENT));
    let retrieved_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&literal_probe_batch_merge_typed_id)
        .send()
        .await?;
    let entity_4 = retrieved_4.body();
    expect_that!(
        entity_4,
        has_attribute("config", any_attribute().value_member(["@value", "a"], 1).without_value_member(["@value", "b"]))
    );
    Ok(())
}
