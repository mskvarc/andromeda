//! A temporal create still records a deleted instance.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    core_context::core_entry,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{
        AttributeKind,
        Context,
        ContextEntry,
        InlineContext,
        NgsiLdMediaType,
        Property,
        evolution::{DeletedInstance, TemporalEntity},
    },
    query::temporal_query,
};

/// Pins: Create or Update Temporal Evolution with an instance whose value is urn:ngsi-ld:null and which carries
/// `deletedAt`, beside an instance with a value, answers 201
/// Clause: 5.5.4, 4.5.7, 5.6.11.4
#[gtest]
#[tokio::test]
async fn n_tmp_ups_9008_a_temporal_create_still_records_a_deleted_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let body_context = broker
        .with_context(Context::Multiple(vec![
            ContextEntry::Inline(r#"{"settings": {"@id": "https://example.org/settings", "@type": "@json"}}"#.parse::<InlineContext>()?),
            core_entry()?,
        ]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let literal_probe_temporal_deleted_id = namespace.urn("LiteralProbe", "temporal-deleted")?;
    let literal_probe_type = namespace.entity_type("LiteralProbe")?;
    let literal_probe_temporal_deleted_evolution = TemporalEntity::builder()
        .id(literal_probe_temporal_deleted_id.clone())
        .entity_type(literal_probe_type)
        .property("config", Property::builder(1).observed_at("2024-01-01T00:00:00Z".parse()?))
        .deleted(
            "config",
            DeletedInstance::builder(AttributeKind::Property, "2024-01-02T00:00:00Z".parse()?)
                .observed_at("2024-01-02T00:00:00Z".parse()?)
                .build(),
        )
        .build()?;
    let answer = body_context
        .with_responses()
        .temporal()
        .create_or_update(&literal_probe_temporal_deleted_evolution)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&literal_probe_temporal_deleted_id)
        .temporal_q(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution(
            "config",
            all!(
                contains(any_attribute().value(1).observed_at("2024-01-01T00:00:00Z".parse()?)),
                contains(any_attribute().value(1)).times(eq(1))
            )
        )
    );
    Ok(())
}
