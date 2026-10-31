//! A capture reads its query under the context of the snapshot.

use andromeda_test_suite::{broker, eventually::retried, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, ContextEntry, Entity, InlineContext, NgsiLdMediaType},
    query::q,
    snapshot::{QueryDocument, Snapshot, SnapshotId, SnapshotStatus},
};

/// Pins: A Snapshot created under a user @context with a query naming a type and a q over an Attribute of that
/// context captures the one Entity meeting the q
/// Clause: 5.16.1.4, 5.5.7, 5.7.2.4, 4.9
#[gtest]
#[tokio::test]
async fn n_snp_cre_8115_a_capture_reads_its_query_under_the_context_of_the_snapshot() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let body_context = broker
        .with_context(Context::Multiple(vec![
            ContextEntry::Reference(IriRefBuf::new("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld".to_owned())?),
            ContextEntry::Inline(
                format!(r#"{{"Vehicle_{namespace}": "https://example.org/{namespace}/Vehicle", "speed": "https://example.org/{namespace}/speed"}}"#)
                    .parse::<InlineContext>()?,
            ),
        ]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let vehicle_fast_id = namespace.urn("Vehicle", "fast")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_slow_id = namespace.urn("Vehicle", "slow")?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let vehicle_fast = Entity::builder()
        .id(vehicle_fast_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 80)
        .build()?;
    body_context.entities().create(&vehicle_fast).send().await?.done()?;
    let vehicle_slow = Entity::builder()
        .id(vehicle_slow_id)
        .entity_type(vehicle_type.clone())
        .property("speed", 20)
        .build()?;
    body_context.entities().create(&vehicle_slow).send().await?.done()?;
    let snapshot = Snapshot::builder()
        .id(format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?)
        .query(
            QueryDocument::builder()
                .entities([vehicle_type.as_str()])
                .q(q::attr("speed").gt(50).build()?)
                .build()?,
        )
        .build()?;
    body_context.snapshots().create(&snapshot).send().await?;
    retried(async || {
        let snapshot_2 = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .snapshots()
            .retrieve(&snapshot_id)
            .send()
            .await?;
        verify_that!(snapshot_2.status(), eq("success".parse::<SnapshotStatus>()?))?;
        Ok(())
    })
    .await?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&snapshot_id)
        .entities()
        .query()
        .entity_type(format!("https://example.org/{namespace}/Vehicle").as_str())
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&vehicle_fast_id)));
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
