//! Every non merge write refuses a json literal holding the null under a key.

use andromeda_test_suite::{
    attribute_matcher::{AttributeMatcher, any_attribute},
    broker::{self, BrokerClient},
    core_context::core_entry,
    entity_matcher::has_attribute,
    namespace::Namespace,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use iri_rs::{Iri, iri};
use ngsi_ld::{
    http::{
        ContextDelivery,
        UncheckedDocument,
        headers::Accept,
        operation::{AppendAttributes, CreateEntity, PartialAttributeUpdate, ReplaceAttribute, ReplaceEntity, UpdateAttributes},
    },
    model::{
        Attribute,
        AttributeName,
        Change,
        Context,
        ContextEntry,
        Entity,
        EntityFragment,
        EntityId,
        EntityTypeName,
        ErrorType,
        InlineContext,
        Lossless,
        NgsiLdMediaType,
        Property,
        RawJson,
    },
};

/// The IRI the `settings` term of the probe context expands to.
const SETTINGS: Iri<&str> = iri!("https://example.org/settings");

/// The two spellings of a JSON literal in a JSON-LD body.
#[derive(Debug, Clone, Copy)]
enum Spelling {
    /// An object under a term the `@context` types `@json`.
    Keyed,
    /// A value object `{"@type": "@json", "@value": ...}`.
    Typed,
}

impl Spelling {
    /// The word the identifiers of the spelling carry.
    const fn word(self) -> &'static str {
        match self {
            Spelling::Keyed => "keyed",
            Spelling::Typed => "typed",
        }
    }

    /// The Property holding the literal `{"a": 1, "b": b}` in this spelling; `b` is JSON text.
    fn property(self, b: &str) -> Result<RawJson> {
        let literal = match self {
            Spelling::Keyed => format!(r#"{{"settings": {{"a": 1, "b": {b}}}}}"#),
            Spelling::Typed => format!(r#"{{"@type": "@json", "@value": {{"a": 1, "b": {b}}}}}"#),
        };
        Ok(format!(r#"{{"type": "Property", "value": {literal}}}"#).parse()?)
    }

    /// What a read of the stored Property holds: the literal as the first write left it.
    fn stored(self) -> AttributeMatcher {
        match self {
            Spelling::Keyed => any_attribute()
                .value_member([SETTINGS.as_str(), "@value", "a"], 1)
                .value_member([SETTINGS.as_str(), "@value", "b"], 2),
            Spelling::Typed => any_attribute().value_member(["@value", "a"], 1).value_member(["@value", "b"], 2),
        }
    }
}

/// Creates the probe entity `id` holding the literal `{"a": 1, "b": 2}` in `spelling`.
async fn create_probe(body_context: &BrokerClient, id: &EntityId, entity_type: &EntityTypeName, spelling: Spelling) -> Result<()> {
    let placeholder = Entity::builder().id(id.clone()).entity_type(entity_type.clone()).build()?;
    // The literal is the subject of the test, so the Property is written as text.
    let document = UncheckedDocument::from_entity(&placeholder, Lossless::Normalized).set_member("config", &spelling.property("2")?);
    let request = body_context
        .unchecked(&CreateEntity::builder().entity(&placeholder).build())?
        .document(&document);
    let answer = body_context.send_unchecked(request).await;
    verify_that!(&answer, answered(StatusCode::CREATED))
}

/// Expects the entity `id` to read as `create_probe` left it.
async fn expect_unchanged(broker: &BrokerClient, id: &EntityId, spelling: Spelling) -> Result<()> {
    let retrieved = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).entities().retrieve(id).send().await?;
    verify_that!(retrieved.body(), has_attribute("config", spelling.stored()))
}

/// Pins: Replace Entity, Update Attributes, Append Attributes, Partial Attribute Update and Replace Attribute each
/// answer 400 `BadRequestData` for a JSON literal, in either spelling, holding urn:ngsi-ld:null under a key, and
/// leave the held literal as it was
/// Clause: 5.5.4, 5.6.2, 5.6.3, 5.6.4, 5.6.18, 5.6.19
#[gtest]
#[tokio::test]
async fn n_cmn_val_9024_every_non_merge_write_refuses_a_json_literal_holding_the_null_under_a_key() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let probe_type = namespace.entity_type("LiteralProbe")?;
    let config: AttributeName = "config".parse()?;
    let settings: InlineContext = format!(r#"{{"settings": {{"@id": "{SETTINGS}", "@type": "@json"}}}}"#).parse()?;
    let body_context = broker
        .with_context(Context::Multiple(vec![ContextEntry::Inline(settings), core_entry()?]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    for spelling in [Spelling::Keyed, Spelling::Typed] {
        let word = spelling.word();
        let null_property = spelling.property(r#""urn:ngsi-ld:null""#)?;

        let replace_entity_id = namespace.urn("LiteralProbe", format!("replace-entity-{word}"))?;
        create_probe(&body_context, &replace_entity_id, &probe_type, spelling).await?;
        let placeholder = Entity::builder().id(replace_entity_id.clone()).entity_type(probe_type.clone()).build()?;
        // Replace Entity names its target in the path only, so the body carries no `id`.
        let document = UncheckedDocument::from_entity(&placeholder, Lossless::Normalized)
            .remove_member("id")
            .set_member("config", &null_property);
        let request = body_context
            .unchecked(&ReplaceEntity::builder().entity(&placeholder).build())?
            .document(&document);
        let answer = body_context.send_unchecked(request).await;
        expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
        expect_unchanged(&broker, &replace_entity_id, spelling).await?;

        let empty = EntityFragment::builder().build()?;
        let fragment_document = UncheckedDocument::empty().set_member("config", &null_property);
        let update_attributes_id = namespace.urn("LiteralProbe", format!("update-attributes-{word}"))?;
        create_probe(&body_context, &update_attributes_id, &probe_type, spelling).await?;
        let request_2 = body_context
            .unchecked(&UpdateAttributes::builder().id(&update_attributes_id).fragment(&empty).build())?
            .document(&fragment_document);
        let answer_2 = body_context.send_unchecked(request_2).await;
        expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
        expect_unchanged(&broker, &update_attributes_id, spelling).await?;

        let append_attributes_id = namespace.urn("LiteralProbe", format!("append-attributes-{word}"))?;
        create_probe(&body_context, &append_attributes_id, &probe_type, spelling).await?;
        let request_3 = body_context
            .unchecked(&AppendAttributes::builder().id(&append_attributes_id).fragment(&empty).build())?
            .document(&fragment_document);
        let answer_3 = body_context.send_unchecked(request_3).await;
        expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
        expect_unchanged(&broker, &append_attributes_id, spelling).await?;

        let attribute_document = UncheckedDocument::try_from(null_property)?;
        let partial_attribute_update_id = namespace.urn("LiteralProbe", format!("partial-attribute-update-{word}"))?;
        create_probe(&body_context, &partial_attribute_update_id, &probe_type, spelling).await?;
        let patch = Change::from(Attribute::from(Property::builder(1).build()?).into_patch());
        let request_4 = body_context
            .unchecked(
                &PartialAttributeUpdate::builder()
                    .id(&partial_attribute_update_id)
                    .name(&config)
                    .attribute(&patch)
                    .build(),
            )?
            .document(&attribute_document);
        let answer_4 = body_context.send_unchecked(request_4).await;
        expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
        expect_unchanged(&broker, &partial_attribute_update_id, spelling).await?;

        let replace_attribute_id = namespace.urn("LiteralProbe", format!("replace-attribute-{word}"))?;
        create_probe(&body_context, &replace_attribute_id, &probe_type, spelling).await?;
        let replacement = Attribute::from(Property::builder(1).build()?);
        let request_5 = body_context
            .unchecked(
                &ReplaceAttribute::builder()
                    .id(&replace_attribute_id)
                    .name(&config)
                    .attribute(&replacement)
                    .build(),
            )?
            .document(&attribute_document);
        let answer_5 = body_context.send_unchecked(request_5).await;
        expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
        expect_unchanged(&broker, &replace_attribute_id, spelling).await?;
    }
    Ok(())
}
