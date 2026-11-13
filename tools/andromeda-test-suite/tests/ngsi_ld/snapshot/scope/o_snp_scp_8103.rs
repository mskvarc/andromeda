//! An add context naming a snapshot is refused and stores nothing.

use andromeda_test_suite::{broker, context_details, namespace::Namespace, snapshot_refusal::refused_on_snapshot};
use googletest::prelude::*;
use http::header::LOCATION;
use ngsi_ld::{
    http::operation::{AddContext, ContextKindFilter, ListContexts},
    model::{Context, ContextEntry, InlineContext},
    snapshot::{QueryDocument, Snapshot, SnapshotId},
};

/// Pins: Add @context and List @contexts with NGSILD-Snapshot naming an existing Snapshot answer 422
/// `OperationNotSupported` with the same header and a detail naming the core, temporal and entity-subscription
/// operations, the refused add answers no Location, and the tenant then lists no hosted @context
/// Clause: 5.5.15, 6.3.22, 5.13.2, 5.13.3
/// Recorded: clause 5.5.15 permits on a Snapshot the operations of the Core and Temporal APIs alone and names no error
/// for another; Andromeda answers `OperationNotSupported`
/// Tenant: private (the case asserts the tenant's whole hosted @context listing; in the shared tenant every other
/// file's entries are listed too)
#[gtest]
#[tokio::test]
async fn o_snp_scp_8103_an_add_context_naming_a_snapshot_is_refused_and_stores_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let front = broker::front()?;
    let broker = broker::private_tenant(&front, &namespace).await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    let context = Context::Single(ContextEntry::Inline(
        format!(r#"{{"Vehicle_{namespace}": "https://test.andromeda/{namespace}/Vehicle"}}"#).parse::<InlineContext>()?,
    ));
    let on_snapshot = broker.on_snapshot(&snapshot_id);
    let added = broker
        .send_unchecked(on_snapshot.unchecked(&AddContext::builder().context(&context).build())?)
        .await;
    expect_that!(&added, refused_on_snapshot(&snapshot_id).without_header(LOCATION));
    let listed = broker.send_unchecked(on_snapshot.unchecked(&ListContexts::builder().build())?).await;
    expect_that!(&listed, refused_on_snapshot(&snapshot_id));
    expect_that!(context_details::listed(&broker, Some(ContextKindFilter::Hosted)).await?, is_empty());
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
