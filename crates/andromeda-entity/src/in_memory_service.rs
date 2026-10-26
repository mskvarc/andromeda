//! An entity service reading the in-memory store, for tests that drive an
//! operation against stored state rather than a recorded one.

use crate::{
    EntityService,
    batch::execution::BatchExecutionConfig,
    change_notifier::ChangeNotifier,
    configured_policy::payload_budget,
    in_memory_store::InMemoryStore,
    port::{
        attribute_history::AttributeDatasetHistory,
        blocking_executor::BlockingExecutor,
        deletion_outline::DeletionOutlineReader,
        reader::EntityReader,
        service_ports::EntityServicePorts,
    },
    recording_ports::{ImmediateRetry, UnusedAttributeHistory, UnusedBlockingExecutor},
};
use andromeda_configuration::schema::entity::EntityConfig;
use andromeda_context_commit::committer::ContextCommitter;
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_retry::retry_delay::RetryDelay;
use std::sync::Arc;

/// An entity service reading and outlining `store`, committing through
/// `writes` and notifying through `notifier`.
///
/// A batch of the size these tests submit is one chunk, and a lost attempt is
/// retried without waiting.
pub(crate) fn in_memory_service(store: &Arc<InMemoryStore>, writes: Arc<dyn ContextCommitter>, notifier: ChangeNotifier) -> EntityService {
    EntityService::new(
        EntityServicePorts {
            reader: Arc::clone(store) as Arc<dyn EntityReader>, // clone: the service reads the store the test inspects
            deletion_outline: Arc::clone(store) as Arc<dyn DeletionOutlineReader>, // clone: the service outlines the store the test inspects
            blocking_executor: Arc::new(UnusedBlockingExecutor) as Arc<dyn BlockingExecutor>,
            retry_delay: Arc::new(ImmediateRetry) as Arc<dyn RetryDelay>,
            attribute_history: Arc::new(UnusedAttributeHistory) as Arc<dyn AttributeDatasetHistory>,
            writes,
        },
        JsonLdEngine::builder().allow_remote(false).build().expect("the offline JSON-LD engine builds"),
        Arc::new(notifier),
        BatchExecutionConfig {
            chunk_concurrency: 1,
            chunk_write_concurrency: 1,
            chunk_size: 64,
            large_threshold: 1024,
            prepare_blocking_threshold: 1024,
            async_side_effect_threshold: None,
        },
        payload_budget(&EntityConfig::default()),
    )
}
