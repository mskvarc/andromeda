//! The wait for a Snapshot to settle: a Snapshot is created in the `preparing` status and settles once its queries
//! have run (clause 5.16.1.4), so a test that writes under it or reads it waits for the status it expects first.

use crate::{broker::BrokerClient, eventually::retried};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    snapshot::{SnapshotId, SnapshotStatus},
};

/// Waits until the Snapshot `id` reports `status`, reading it every poll interval up to the delivery deadline.
///
/// # Errors
///
/// Returns the failure of the last read when the Snapshot never reports `status`.
pub async fn await_status(broker: &BrokerClient, id: &SnapshotId, status: SnapshotStatus) -> Result<()> {
    retried(async || {
        let snapshot = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).snapshots().retrieve(id).send().await?;
        verify_that!(snapshot.status(), eq(status))?;
        Ok(())
    })
    .await
}
