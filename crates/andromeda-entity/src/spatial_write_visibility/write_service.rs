//! The entity service the tests write through.

use crate::{
    EntityService,
    change_notifier::{ChangeNotifier, SubscriptionMatching},
    in_memory_service::in_memory_service,
    in_memory_store::InMemoryStore,
    spatial_indexing_committer::SpatialIndexingCommitter,
    spatial_write_visibility::geo_query::placement,
};
use andromeda_context_commit::committer::ContextCommitter;
use andromeda_spatial_index::entity::index::SpatialEntityIndex;
use std::sync::Arc;

/// An entity service over an empty in-memory store whose every commit keeps
/// `index` in step, as `commit-synchronous` consistency composes it, with no
/// change sink: nothing applies a committed change asynchronously.
pub(super) fn indexed_service(index: &Arc<SpatialEntityIndex>) -> EntityService {
    let store = Arc::new(InMemoryStore::default());
    // clone: the committer and the query service share the index; the committer and the service share the store
    let committer = SpatialIndexingCommitter::new(Arc::clone(&store) as Arc<dyn ContextCommitter>, Arc::clone(index), placement());
    in_memory_service(&store, Arc::new(committer), ChangeNotifier::new(None, SubscriptionMatching::Absent))
}
