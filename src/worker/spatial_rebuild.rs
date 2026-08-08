//! Periodic full rebuild of the entity spatial index.
//!
//! The incremental sync in [`crate::worker::spatial_sync`] listens on a
//! `broadcast` channel, which drops events when a consumer falls behind. This
//! is the other half of that bargain: every partition is rebuilt from the
//! store on a timer, so drift from a dropped event or a restarted worker heals
//! on its own rather than persisting until the process ends.

use andromeda_postgresql_context_store::spatial_warmup::warmup_location;
use andromeda_spatial_index::entity::index::SpatialEntityIndex;
use andromeda_tenancy::tenant_ref::TenantRef;
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};
use tokio::time::{MissedTickBehavior, interval};
use tracing::{debug, warn};

/// Rebuild every partition on a timer until the process ends.
pub async fn periodic_rebuild(index: Arc<SpatialEntityIndex>, pool: PgPool, tenant: TenantRef, period: Duration) {
    let mut ticker = interval(period);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    // The first tick fires immediately; the warm-up at startup has already
    // done that work.
    ticker.tick().await;
    loop {
        ticker.tick().await;
        match warmup_location(&index, &pool, &tenant).await {
            Ok(stats) => debug!(
                partition_count = stats.partition_count,
                total_entries = stats.total_entries,
                "spatial periodic rebuild complete"
            ),
            Err(error) => warn!(error = %error, "spatial periodic rebuild failed"),
        }
    }
}
