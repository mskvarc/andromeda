//! Construction of one tenant's Snapshot preparation and the Snapshot service
//! that starts it for each created or cloned Snapshot.

use crate::tenant_factory::{TenantFactoryConfig, storage::TenantStorage, tasks::snapshot_preparation_start::slotted_preparation_start};
use andromeda_configuration::human_byte_size::HumanByteSize;
use andromeda_context_commit::committer::ContextCommitter;
use andromeda_entity::configured_policy::payload_budget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_snapshot::{
    executor::{SnapshotPreparation, SnapshotPreparationConfig, capture_sources::SnapshotCaptureSources},
    service::{SnapshotService, SnapshotServiceConfig},
    status_delivery_signal::StatusDeliverySignal,
};
use andromeda_task_runtime::keyed_slots::KeyedSlots;
use std::{num::NonZeroUsize, sync::Arc};

/// Compose the snapshot preparation operation capturing from `sources`, and
/// the service that starts a created Snapshot's preparation on `slots`.
pub(super) fn compose_snapshot_services(
    config: &TenantFactoryConfig,
    storage: &TenantStorage,
    writes: &Arc<dyn ContextCommitter>,
    sources: SnapshotCaptureSources,
    slots: KeyedSlots<Arc<Identifier>>,
    delivery_signal: Arc<dyn StatusDeliverySignal>,
) -> (Arc<SnapshotPreparation>, Arc<SnapshotService>) {
    let snapshot = &config.snapshot;
    let preparation = Arc::new(SnapshotPreparation::new(
        sources,
        Arc::clone(&storage.snapshot_store),
        Arc::clone(writes),
        SnapshotPreparationConfig {
            capture_page_size: snapshot.preparation.page_size.get(),
            max_per_pass: snapshot.preparation.max_concurrent.get(),
            lease_duration: snapshot.preparation.lease_timeout.duration(),
            tenant_count_quota: snapshot.quota.max_per_tenant.limit().map(NonZeroUsize::get),
            tenant_byte_quota: snapshot.quota.max_storage_per_tenant.limit().map(HumanByteSize::as_u64),
            attribute_nesting: payload_budget(&config.request_limits).attribute_nesting,
        },
    ));
    let service = Arc::new(SnapshotService::new(
        Arc::clone(&storage.snapshot_store),
        Some(slotted_preparation_start(slots, Arc::clone(&preparation), Arc::clone(&delivery_signal))),
        Some(delivery_signal),
        SnapshotServiceConfig {
            default_lifetime: snapshot.default_lifetime.duration(),
            maximum_lifetime: snapshot.max_lifetime.duration(),
            scoped_operation_wait: snapshot.preparation.scoped_operation_timeout.duration(),
            tenant_count_quota: snapshot.quota.max_per_tenant.limit().map(NonZeroUsize::get),
            tenant_byte_quota: snapshot.quota.max_storage_per_tenant.limit().map(HumanByteSize::as_u64),
        },
    ));
    (preparation, service)
}
