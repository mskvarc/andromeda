//! The entity service the tests write through.

use crate::{
    EntityService,
    batch::execution::BatchExecutionConfig,
    change_notifier::ChangeNotifier,
    error::EntityError,
    port::{
        attribute_history::AttributeDatasetHistory,
        blocking_executor::{BlockingExecutor, BlockingTask},
        reader::EntityReader,
        retry_delay::RetryDelay,
        service_ports::EntityServicePorts,
    },
    spatial_indexing_committer::SpatialIndexingCommitter,
    spatial_write_visibility::{geo_query::placement, in_memory_store::InMemoryStore},
};
use andromeda_context_commit::committer::ContextCommitter;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use andromeda_spatial_index::entity::index::SpatialEntityIndex;
use async_trait::async_trait;
use std::{num::NonZeroUsize, sync::Arc, time::Duration};

/// A retry delay that does not wait.
struct ImmediateRetry;

#[async_trait]
impl RetryDelay for ImmediateRetry {
    async fn wait(&self, _delay: Duration) {}
}

/// A blocking executor the single-entity paths never reach.
struct UnusedBlockingExecutor;

#[async_trait]
impl BlockingExecutor for UnusedBlockingExecutor {
    async fn execute(&self, _tasks: Vec<Box<dyn BlockingTask>>, _concurrency: NonZeroUsize) -> Result<(), EntityError> {
        Err(EntityError::from(PersistenceFailure::Unavailable))
    }
}

/// A dataset history that holds no Temporal Evolution.
struct NoAttributeHistory;

#[async_trait]
impl AttributeDatasetHistory for NoAttributeHistory {
    async fn dataset_identifiers(
        &self,
        _target: &ContextTarget,
        _entity: &Identifier,
        _attribute: &AttributeName,
    ) -> Result<Vec<Option<Identifier>>, EntityError> {
        Ok(Vec::new())
    }
}

/// An entity service committing through `writes` and reading `store`, with no
/// change sink: nothing applies a committed change asynchronously.
fn entity_service(store: &Arc<InMemoryStore>, writes: Arc<dyn ContextCommitter>) -> EntityService {
    EntityService::new(
        EntityServicePorts {
            reader: Arc::clone(store) as Arc<dyn EntityReader>, // clone: the service reads the store the test inspects
            blocking_executor: Arc::new(UnusedBlockingExecutor) as Arc<dyn BlockingExecutor>,
            retry_delay: Arc::new(ImmediateRetry) as Arc<dyn RetryDelay>,
            attribute_history: Arc::new(NoAttributeHistory) as Arc<dyn AttributeDatasetHistory>,
            writes,
        },
        JsonLdEngine::builder().allow_remote(false).build().expect("the offline JSON-LD engine builds"),
        Arc::new(ChangeNotifier::new(None, None)),
        BatchExecutionConfig {
            chunk_concurrency: 1,
            chunk_write_concurrency: 1,
            chunk_size: 1,
            large_threshold: 1,
            prepare_blocking_threshold: 1024,
            async_side_effect_threshold: None,
        },
    )
}

/// An entity service over an empty in-memory store whose every commit keeps
/// `index` in step, as `commit-synchronous` consistency composes it.
pub(super) fn indexed_service(index: &Arc<SpatialEntityIndex>) -> EntityService {
    let store = Arc::new(InMemoryStore::default());
    // clone: the committer and the query service share the index; the committer and the service share the store
    let committer = SpatialIndexingCommitter::new(Arc::clone(&store) as Arc<dyn ContextCommitter>, Arc::clone(index), placement());
    entity_service(&store, Arc::new(committer))
}
