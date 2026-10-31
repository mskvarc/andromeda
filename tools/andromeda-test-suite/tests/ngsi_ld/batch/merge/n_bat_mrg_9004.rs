//! A value key a literal key and a language are deleted as merge entity does.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    core_context::core_entry,
    entity_matcher::has_attribute,
    language_map_matcher::has_language_map,
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
    model::{
        Attribute,
        Context,
        ContextEntry,
        Entity,
        EntityFragment,
        InlineContext,
        LanguageMap,
        LanguageProperty,
        Lossless,
        NgsiLdMediaType,
        Property,
        RawJson,
        Value,
        ValuePatch,
    },
};

/// Pins: Merge Entity and Batch Entity Merge setting a key of a structured value, a key of a JSON literal and one
/// language of a `LanguageProperty` to the NGSI-LD Null answer 204, and both entities keep only the other keys and
/// language
/// Clause: 5.6.20.4, 5.6.17.4, 5.5.12
#[gtest]
#[tokio::test]
async fn n_bat_mrg_9004_a_value_key_a_literal_key_and_a_language_are_deleted_as_merge_entity_does() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let null_deep_single_id = namespace.urn("NullDeep", "single")?;
    let null_deep_type = namespace.entity_type("NullDeep")?;
    let null_deep_batched_id = namespace.urn("NullDeep", "batched")?;
    let settings: InlineContext = r#"{"settings": {"@id": "https://example.org/settings", "@type": "@json"}}"#.parse()?;
    let body_context = broker
        .with_context(Context::Multiple(vec![ContextEntry::Inline(settings), core_entry()?]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    for id in [&null_deep_single_id, &null_deep_batched_id] {
        let entity = Entity::builder()
            .id(id.clone())
            .entity_type(null_deep_type.clone())
            .property(
                "address",
                Property::builder(object([
                    ("street", Value::from("a")),
                    ("city", Value::from("b")),
                    ("country", Value::from("c")),
                ])),
            )
            .language_property(
                "label",
                LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "hi"), ("de", "hallo")])?),
            )
            .property(
                "config",
                Property::builder(object([("settings", object([("a", Value::from(1)), ("b", Value::from(2))]))])),
            )
            .build()?;
        body_context.entities().create(&entity).send().await?.done()?;
    }
    let address = || ValuePatch::new().set("street", "z").delete("country");
    // The `@context` types `settings` as `@json`, so the literal's keys are written as the keys of a plain object.
    let config = || ValuePatch::new().merge("settings", ValuePatch::new().delete("b"));
    // Clause 5.5.12: the model writes the deletion of a key of a value, not of a language map entry, so `label` is
    // written as text.
    let label: RawJson = r#"{"type": "LanguageProperty", "languageMap": {"de": "urn:ngsi-ld:null"}}"#.parse()?;
    let document = |rendered: RawJson| -> Result<UncheckedDocument> { Ok(UncheckedDocument::try_from(rendered)?.set_member("label", &label)) };
    let single = EntityFragment::builder()
        .patch("address", Attribute::from(Property::merge_builder(address()).build()?))
        .patch("config", Attribute::from(Property::merge_builder(config()).build()?))
        .build()?;
    let request = body_context
        .unchecked(&MergeEntity::builder().id(&null_deep_single_id).fragment(&single).build())?
        .document(&document(single.render(Lossless::Normalized))?);
    let answer = body_context.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let batched = [EntityFragment::for_entity(null_deep_batched_id.clone(), null_deep_type.clone())
        .patch("address", Attribute::from(Property::merge_builder(address()).build()?))
        .patch("config", Attribute::from(Property::merge_builder(config()).build()?))
        .build()?];
    let request_2 = body_context
        .unchecked(&BatchMerge::builder().fragments(&batched).build())?
        .documents(&[document(batched[0].render(Lossless::Normalized))?]);
    let answer_2 = body_context.send_unchecked(request_2).await;
    expect_that!(&answer_2, answered(StatusCode::NO_CONTENT));
    // Merge Entity first, then Batch Entity Merge, which does what Merge Entity does.
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    for id in [&null_deep_single_id, &null_deep_batched_id] {
        let retrieved = json.entities().retrieve(id).send().await?;
        let entity = retrieved.body();
        expect_that!(
            entity,
            has_attribute(
                "address",
                property()
                    .value_member(["street"], "z")
                    .value_member(["city"], "b")
                    .without_value_member(["country"])
            )
        );
        expect_that!(entity, has_language_map("label", &[("en", "hi")]));
        // Without the `@context` that defines `settings`, the literal is answered under its expanded term (clause 4.4).
        expect_that!(
            entity,
            has_attribute(
                "config",
                property()
                    .value_member(["https://example.org/settings", "@value", "a"], 1)
                    .without_value_member(["https://example.org/settings", "@value", "b"])
            )
        );
    }
    Ok(())
}
