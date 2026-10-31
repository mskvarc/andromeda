//! A clone succeeds while other snapshots of its tenant are deleted and created.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, snapshot_capture::await_status};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{Entity, Input, NgsiLdMediaType},
    snapshot::{QueryDocument, SettledStatus, Snapshot, SnapshotClone, SnapshotId, SnapshotStatus},
};
use std::iter;

/// Pins: Clone Snapshot ends `success` with every Entity of its source, and the clone takes a new Entity, while
/// other Snapshots of the same tenant are deleted and created during the copy
/// Clause: 5.16.2.4
/// Tenant: private (the test controls every Snapshot of its tenant)
#[gtest]
#[tokio::test]
async fn n_snp_cln_9169_a_clone_succeeds_while_other_snapshots_of_its_tenant_are_deleted_and_created() -> Result<()> {
    let namespace = Namespace::fresh();
    let tenant = broker::private_tenant(&broker::front()?, &namespace).await?;
    let device_type = namespace.entity_type("Device")?;
    let devices = (1..=200)
        .map(|number| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Device", number)?)
                .entity_type(device_type.clone())
                .property("temperature", number)
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    for chunk in devices.chunks(100) {
        tenant.entities().batch_create(chunk).send().await?.done()?;
    }
    let snapshot_id = |name: String| format!("urn:ngsi-ld:Snapshot:{namespace}-{name}").parse::<SnapshotId>();
    let snapshot_of_devices = |id: &SnapshotId| -> Result<Snapshot<Input>> {
        Ok(Snapshot::builder()
            .id(id.clone())
            .query(QueryDocument::builder().entities([device_type.as_str()]).build()?)
            .build()?)
    };
    // A deletion commits inside the copy only on some rounds, so the clone runs three times.
    for round in 1..=3 {
        let source_id = snapshot_id(format!("{round}-source"))?;
        let clone_id = snapshot_id(format!("{round}-clone"))?;
        let other_ids = (1..=8).map(|n| snapshot_id(format!("{round}-other-{n}"))).collect::<Result<Vec<_>, _>>()?;
        let new_ids = (1..=2).map(|n| snapshot_id(format!("{round}-new-{n}"))).collect::<Result<Vec<_>, _>>()?;
        for id in iter::once(&source_id).chain(&other_ids) {
            tenant.snapshots().create(&snapshot_of_devices(id)?).send().await?;
        }
        for id in iter::once(&source_id).chain(&other_ids) {
            await_status(&tenant, id, SnapshotStatus::Settled(SettledStatus::Success)).await?;
        }
        let new_snapshots = new_ids.iter().map(snapshot_of_devices).collect::<Result<Vec<_>>>()?;
        let clone = SnapshotClone::builder().id(clone_id.clone()).build()?;
        let (cloned, deleted, created) = tokio::join!(
            async { tenant.snapshots().clone_snapshot(&source_id, &clone).send().await },
            async {
                for id in &other_ids {
                    tenant.snapshots().delete(id).send().await?;
                }
                Result::<()>::Ok(())
            },
            async {
                for snapshot in &new_snapshots {
                    tenant.snapshots().create(snapshot).send().await?;
                }
                Result::<()>::Ok(())
            },
        );
        cloned?;
        deleted?;
        created?;
        await_status(&tenant, &clone_id, SnapshotStatus::Settled(SettledStatus::Success)).await?;
        let page = tenant
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .on_snapshot(&clone_id)
            .entities()
            .query()
            .entity_type(&device_type)
            .count(ResultsCount::Include)
            .send()
            .await?;
        expect_that!(page.total(), some(eq(200)));
        // The clone's identity sequences continue past the copied keys, so a new Entity is stored.
        let added = Entity::builder()
            .id(namespace.urn("Device", format!("added-{round}"))?)
            .entity_type(device_type.clone())
            .property("temperature", 0)
            .build()?;
        tenant.on_snapshot(&clone_id).entities().create(&added).send().await?.done()?;
        for id in [&source_id, &clone_id].into_iter().chain(&new_ids) {
            tenant.snapshots().delete(id).send().await?;
        }
    }
    Ok(())
}
