//! A temporal capture stores the history under the iris of its context.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    core_context::core_entry,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    outcome::Upserted,
    snapshot_capture::await_status,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{
        AttributeName,
        Context,
        ContextEntry,
        EntityTypeName,
        InlineContext,
        NgsiLdMediaType,
        OneOrMany,
        Property,
        evolution::{TemporalAttributeName, TemporalEntity},
    },
    query::temporal_query::{self, TemporalQuery},
    snapshot::{QueryDocument, SettledStatus, Snapshot, SnapshotId, SnapshotStatus},
};

/// Pins: A Snapshot created under a user @context with a temporal query over a type of that context captures the
/// Temporal Evolution under the IRIs the context gives the type and the Attribute: a temporal query under the
/// Snapshot naming those IRIs answers the Entity with both instances
/// Clause: 5.16.1.4, 5.5.7, 5.7.4.4
#[gtest]
#[tokio::test]
async fn n_snp_cre_8107_a_temporal_capture_stores_the_history_under_the_iris_of_its_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let inline = format!(r#"{{"Vehicle_{namespace}": "https://example.org/{namespace}/Vehicle", "speed": "https://example.org/{namespace}/speed"}}"#)
        .parse::<InlineContext>()?;
    let context = Context::Multiple(vec![core_entry()?, ContextEntry::Inline(inline)]);
    let body_context = broker.with_context(context)?.with_context_delivery(ContextDelivery::LdJsonBody);
    let evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(30).observed_at("2025-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(40).observed_at("2025-01-01T01:00:00Z".parse()?))
        .build()?;
    body_context.temporal().create_or_update(&evolution).send().await?.created()?;
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .temporal_query(
            QueryDocument::temporal_builder(TemporalQuery::after("2024-01-01T00:00:00Z".parse()?))
                .entities([vehicle_type.as_str()])
                .build()?,
        )
        .build()?;
    body_context.snapshots().create(&snapshot).send().await?;
    await_status(&broker, &snapshot_id, SnapshotStatus::Settled(SettledStatus::Success)).await?;
    let type_iri = format!("https://example.org/{namespace}/Vehicle").parse::<EntityTypeName>()?;
    let speed_iri = format!("https://example.org/{namespace}/speed");
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&snapshot_id)
        .temporal()
        .query(temporal_query::after("2024-01-01T00:00:00Z".parse()?))
        .entity_type(&type_iri)
        .attrs(vec![TemporalAttributeName::Attribute(speed_iri.parse::<AttributeName>()?)])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    let captured = page.items().first().or_fail()?;
    expect_that!(captured.id, eq(&vehicle_id));
    expect_that!(captured.entity_type, eq(&OneOrMany::one(type_iri.clone())));
    expect_that!(
        captured,
        has_evolution(
            &speed_iri,
            all!(len(eq(2)), contains(any_attribute().value(30)), contains(any_attribute().value(40)))
        )
    );
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
