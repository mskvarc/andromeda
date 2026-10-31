//! Every batch write refuses only the entity holding a null keyed json literal.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    core_context::core_entry,
    entity_matcher::has_attribute,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    partial_outcome::reported,
    problem::answered,
    report_matcher::batch_error,
    value::object,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        BatchOperationResult,
        BatchOutcome,
        ContextDelivery,
        UncheckedDocument,
        headers::Accept,
        operation::{BatchCreate, BatchUpsert, UpsertMode},
    },
    model::{
        Attribute,
        Context,
        ContextEntry,
        Entity,
        EntityFragment,
        ErrorType,
        InlineContext,
        Lossless,
        NgsiLdMediaType,
        Property,
        RawJson,
        Value,
        ValuePatch,
    },
};

/// Pins: Batch create, upsert, upsert in update mode and update each refuse, as its own `BadRequestData` error,
/// only the Entity whose JSON literal holds urn:ngsi-ld:null under a key, leave it absent or as held, and write its
/// neighbour
/// Clause: 5.5.4, 5.6.7.4, 5.6.8.4, 5.6.9.4, 5.2.17
#[gtest]
#[tokio::test]
async fn n_bat_res_9011_every_batch_write_refuses_only_the_entity_holding_a_null_keyed_json_literal() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let body_context = broker
        .with_context(Context::Multiple(vec![
            ContextEntry::Inline(r#"{"settings": {"@id": "https://example.org/settings", "@type": "@json"}}"#.parse::<InlineContext>()?),
            core_entry()?,
        ]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let literal_probe_batch_create_refused_id = namespace.urn("LiteralProbe", "batch-create-refused")?;
    let literal_probe_type = namespace.entity_type("LiteralProbe")?;
    let literal_probe_batch_create_written_id = namespace.urn("LiteralProbe", "batch-create-written")?;
    let literal_probe_batch_upsert_refused_id = namespace.urn("LiteralProbe", "batch-upsert-refused")?;
    let literal_probe_batch_upsert_written_id = namespace.urn("LiteralProbe", "batch-upsert-written")?;
    let literal_probe_batch_upsert_update_refused_id = namespace.urn("LiteralProbe", "batch-upsert-update-refused")?;
    let literal_probe_batch_upsert_update_written_id = namespace.urn("LiteralProbe", "batch-upsert-update-written")?;
    let literal_probe_batch_update_refused_id = namespace.urn("LiteralProbe", "batch-update-refused")?;
    let literal_probe_batch_update_written_id = namespace.urn("LiteralProbe", "batch-update-written")?;
    let literal_probe_batch_create_refused = Entity::builder()
        .id(literal_probe_batch_create_refused_id.clone())
        .entity_type(literal_probe_type.clone())
        .build()?;
    // The model refuses it (a request cannot hold the NGSI-LD Null in a member of `value` at config), so `config` is written as text.
    let config_2: RawJson = r#"{"type": "Property", "value": {"settings": {"a": 1, "b": "urn:ngsi-ld:null"}}}"#.parse()?;
    let document = UncheckedDocument::from_entity(&literal_probe_batch_create_refused, Lossless::Normalized).set_member("config", &config_2);
    let literal_probe_batch_create_written = Entity::builder()
        .id(literal_probe_batch_create_written_id.clone())
        .entity_type(literal_probe_type.clone())
        .property("config", object([("settings", object([("a", Value::from(3))]))]))
        .build()?;
    let document_2 = UncheckedDocument::from_entity(&literal_probe_batch_create_written, Lossless::Normalized);
    let entities = [literal_probe_batch_create_refused, literal_probe_batch_create_written];
    let request = body_context
        .unchecked(&BatchCreate::builder().entities(&entities).build())?
        .documents(&[document, document_2]);
    let answer = body_context.send_unchecked(request).await;
    let report: BatchOperationResult = reported(answer)?;
    expect_that!(&report.success, len(eq(1)));
    expect_that!(&report.success, nth(0, eq(&literal_probe_batch_create_written_id)));
    expect_that!(&report.errors, len(eq(1)));
    expect_that!(
        &report.errors,
        nth(
            0,
            batch_error().entity(&literal_probe_batch_create_refused_id).problem(ErrorType::BadRequestData)
        )
    );
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&literal_probe_batch_create_refused_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&literal_probe_batch_create_written_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute(
            "config",
            any_attribute()
                .value_member(["https://example.org/settings", "@value", "a"], 3)
                .without_value_member(["https://example.org/settings", "@value", "b"])
        )
    );
    let literal_probe_batch_upsert_refused = Entity::builder()
        .id(literal_probe_batch_upsert_refused_id.clone())
        .entity_type(literal_probe_type.clone())
        .property("config", object([("settings", object([("a", Value::from(1)), ("b", Value::from(2))]))]))
        .build()?;
    body_context.entities().create(&literal_probe_batch_upsert_refused).send().await?.done()?;
    let literal_probe_batch_upsert_written = Entity::builder()
        .id(literal_probe_batch_upsert_written_id.clone())
        .entity_type(literal_probe_type.clone())
        .property("config", object([("settings", object([("a", Value::from(1)), ("b", Value::from(2))]))]))
        .build()?;
    body_context.entities().create(&literal_probe_batch_upsert_written).send().await?.done()?;
    let literal_probe_batch_upsert_refused_2 = Entity::builder()
        .id(literal_probe_batch_upsert_refused_id.clone())
        .entity_type(literal_probe_type.clone())
        .build()?;
    // The model refuses it (a request cannot hold the NGSI-LD Null in a member of `value` at config), so `config` is written as text.
    let config_3: RawJson = r#"{"type": "Property", "value": {"settings": {"a": 1, "b": "urn:ngsi-ld:null"}}}"#.parse()?;
    let document_3 = UncheckedDocument::from_entity(&literal_probe_batch_upsert_refused_2, Lossless::Normalized).set_member("config", &config_3);
    let literal_probe_batch_upsert_written_2 = Entity::builder()
        .id(literal_probe_batch_upsert_written_id.clone())
        .entity_type(literal_probe_type.clone())
        .property("config", object([("settings", object([("a", Value::from(3))]))]))
        .build()?;
    let document_4 = UncheckedDocument::from_entity(&literal_probe_batch_upsert_written_2, Lossless::Normalized);
    let entities_2 = [literal_probe_batch_upsert_refused_2, literal_probe_batch_upsert_written_2];
    let request_2 = body_context
        .unchecked(&BatchUpsert::builder().entities(&entities_2).build())?
        .documents(&[document_3, document_4]);
    let answer_3 = body_context.send_unchecked(request_2).await;
    let report_2: BatchOperationResult = reported(answer_3)?;
    expect_that!(&report_2.success, len(eq(1)));
    expect_that!(&report_2.success, nth(0, eq(&literal_probe_batch_upsert_written_id)));
    expect_that!(&report_2.errors, len(eq(1)));
    expect_that!(
        &report_2.errors,
        nth(
            0,
            batch_error().entity(&literal_probe_batch_upsert_refused_id).problem(ErrorType::BadRequestData)
        )
    );
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&literal_probe_batch_upsert_refused_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(
        entity_2,
        has_attribute(
            "config",
            any_attribute()
                .value_member(["https://example.org/settings", "@value", "a"], 1)
                .value_member(["https://example.org/settings", "@value", "b"], 2)
        )
    );
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&literal_probe_batch_upsert_written_id)
        .send()
        .await?;
    let entity_3 = retrieved_3.body();
    expect_that!(
        entity_3,
        has_attribute(
            "config",
            any_attribute()
                .value_member(["https://example.org/settings", "@value", "a"], 3)
                .without_value_member(["https://example.org/settings", "@value", "b"])
        )
    );
    let literal_probe_batch_upsert_update_refused = Entity::builder()
        .id(literal_probe_batch_upsert_update_refused_id.clone())
        .entity_type(literal_probe_type.clone())
        .property("config", object([("settings", object([("a", Value::from(1)), ("b", Value::from(2))]))]))
        .build()?;
    body_context
        .entities()
        .create(&literal_probe_batch_upsert_update_refused)
        .send()
        .await?
        .done()?;
    let literal_probe_batch_upsert_update_written = Entity::builder()
        .id(literal_probe_batch_upsert_update_written_id.clone())
        .entity_type(literal_probe_type.clone())
        .property("config", object([("settings", object([("a", Value::from(1)), ("b", Value::from(2))]))]))
        .build()?;
    body_context
        .entities()
        .create(&literal_probe_batch_upsert_update_written)
        .send()
        .await?
        .done()?;
    let literal_probe_batch_upsert_update_refused_2 = Entity::builder()
        .id(literal_probe_batch_upsert_update_refused_id.clone())
        .entity_type(literal_probe_type.clone())
        .build()?;
    // The model refuses it (a request cannot hold the NGSI-LD Null in a member of `value` at config), so `config` is written as text.
    let config_4: RawJson = r#"{"type": "Property", "value": {"settings": {"a": 1, "b": "urn:ngsi-ld:null"}}}"#.parse()?;
    let document_5 = UncheckedDocument::from_entity(&literal_probe_batch_upsert_update_refused_2, Lossless::Normalized).set_member("config", &config_4);
    let literal_probe_batch_upsert_update_written_2 = Entity::builder()
        .id(literal_probe_batch_upsert_update_written_id.clone())
        .entity_type(literal_probe_type.clone())
        .property("config", object([("settings", object([("a", Value::from(3))]))]))
        .build()?;
    let document_6 = UncheckedDocument::from_entity(&literal_probe_batch_upsert_update_written_2, Lossless::Normalized);
    let entities_3 = [literal_probe_batch_upsert_update_refused_2, literal_probe_batch_upsert_update_written_2];
    let request_3 = body_context
        .unchecked(&BatchUpsert::builder().entities(&entities_3).mode(UpsertMode::Update).build())?
        .documents(&[document_5, document_6]);
    let answer_4 = body_context.send_unchecked(request_3).await;
    let report_3: BatchOperationResult = reported(answer_4)?;
    expect_that!(&report_3.success, len(eq(1)));
    expect_that!(&report_3.success, nth(0, eq(&literal_probe_batch_upsert_update_written_id)));
    expect_that!(&report_3.errors, len(eq(1)));
    expect_that!(
        &report_3.errors,
        nth(
            0,
            batch_error()
                .entity(&literal_probe_batch_upsert_update_refused_id)
                .problem(ErrorType::BadRequestData)
        )
    );
    let retrieved_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&literal_probe_batch_upsert_update_refused_id)
        .send()
        .await?;
    let entity_4 = retrieved_4.body();
    expect_that!(
        entity_4,
        has_attribute(
            "config",
            any_attribute()
                .value_member(["https://example.org/settings", "@value", "a"], 1)
                .value_member(["https://example.org/settings", "@value", "b"], 2)
        )
    );
    let retrieved_5 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&literal_probe_batch_upsert_update_written_id)
        .send()
        .await?;
    let entity_5 = retrieved_5.body();
    expect_that!(
        entity_5,
        has_attribute(
            "config",
            any_attribute()
                .value_member(["https://example.org/settings", "@value", "a"], 3)
                .without_value_member(["https://example.org/settings", "@value", "b"])
        )
    );
    let literal_probe_batch_update_refused = Entity::builder()
        .id(literal_probe_batch_update_refused_id.clone())
        .entity_type(literal_probe_type.clone())
        .property("config", object([("settings", object([("a", Value::from(1)), ("b", Value::from(2))]))]))
        .build()?;
    body_context.entities().create(&literal_probe_batch_update_refused).send().await?.done()?;
    let literal_probe_batch_update_written = Entity::builder()
        .id(literal_probe_batch_update_written_id.clone())
        .entity_type(literal_probe_type.clone())
        .property("config", object([("settings", object([("a", Value::from(1)), ("b", Value::from(2))]))]))
        .build()?;
    body_context.entities().create(&literal_probe_batch_update_written).send().await?.done()?;
    let literal_probe_batch_update_refused_fragment = EntityFragment::for_entity(literal_probe_batch_update_refused_id.clone(), literal_probe_type.clone())
        .patch(
            "config",
            Attribute::from(Property::merge_builder(ValuePatch::new().merge("settings", ValuePatch::new().set("a", 1).delete("b"))).build()?),
        )
        .build()?;
    let literal_probe_batch_update_written_fragment = EntityFragment::for_entity(literal_probe_batch_update_written_id.clone(), literal_probe_type.clone())
        .property("config", object([("settings", object([("a", Value::from(3))]))]))
        .build()?;
    let fragments = [literal_probe_batch_update_refused_fragment, literal_probe_batch_update_written_fragment];
    let received = body_context.with_responses().entities().batch_update(&fragments).send().await?;
    expect_that!(received.status, eq(StatusCode::MULTI_STATUS));
    let report_4 = match received.outcome {
        BatchOutcome::Partial(report) => Some(report),
        BatchOutcome::Done => None,
    }
    .or_fail()?;
    expect_that!(&report_4.success, len(eq(1)));
    expect_that!(&report_4.success, nth(0, eq(&literal_probe_batch_update_written_id)));
    expect_that!(&report_4.errors, len(eq(1)));
    expect_that!(
        &report_4.errors,
        nth(
            0,
            batch_error().entity(&literal_probe_batch_update_refused_id).problem(ErrorType::BadRequestData)
        )
    );
    let retrieved_6 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&literal_probe_batch_update_refused_id)
        .send()
        .await?;
    let entity_6 = retrieved_6.body();
    expect_that!(
        entity_6,
        has_attribute(
            "config",
            any_attribute()
                .value_member(["https://example.org/settings", "@value", "a"], 1)
                .value_member(["https://example.org/settings", "@value", "b"], 2)
        )
    );
    let retrieved_7 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&literal_probe_batch_update_written_id)
        .send()
        .await?;
    let entity_7 = retrieved_7.body();
    expect_that!(
        entity_7,
        has_attribute(
            "config",
            any_attribute()
                .value_member(["https://example.org/settings", "@value", "a"], 3)
                .without_value_member(["https://example.org/settings", "@value", "b"])
        )
    );
    Ok(())
}
