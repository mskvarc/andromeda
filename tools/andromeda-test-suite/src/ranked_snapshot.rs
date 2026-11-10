//! Snapshots a quota test ranks against each other: each captures a type of its namespace no Entity has, so its
//! preparation ends at once, and they differ only in the `snapshotPriority` and `snapshotLifetime` the broker ranks its
//! eviction victims by.

use crate::namespace::Namespace;
use googletest::Result as TestResult;
use ngsi_ld::{
    model::{Duration, Input},
    snapshot::{QueryDocument, Snapshot, SnapshotId},
};

/// The identifier `urn:ngsi-ld:Snapshot:{namespace}-{name}`.
///
/// # Errors
///
/// Returns the error of an identifier that does not parse.
pub fn ranked_id(namespace: &Namespace, name: &str) -> TestResult<SnapshotId> {
    Ok(format!("urn:ngsi-ld:Snapshot:{namespace}-{name}").parse::<SnapshotId>()?)
}

/// The Snapshot `id` of `priority`, living `lifetime`, that captures the Vehicles of `namespace`.
///
/// # Errors
///
/// Returns the error of an entity type or a Snapshot that does not build.
pub fn ranked_snapshot(namespace: &Namespace, id: &SnapshotId, priority: u8, lifetime: Duration) -> TestResult<Snapshot<Input>> {
    let vehicle_type = namespace.entity_type("Vehicle")?;
    Ok(Snapshot::builder()
        .id(id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .lifetime(lifetime)
        .priority(priority)
        .build()?)
}
