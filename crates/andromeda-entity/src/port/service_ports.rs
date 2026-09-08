//! The caller-owned ports entity policy is constructed over.

use crate::port::{attribute_history::AttributeDatasetHistory, blocking_executor::BlockingExecutor, reader::EntityReader, retry_delay::RetryDelay};
use andromeda_context_commit::committer::ContextCommitter;
use std::sync::Arc;

/// The ports entity policy reaches storage and the runtime through.
///
/// The composition layer builds them as one set and they never vary
/// independently: policy that can read entities but cannot commit them, or can
/// commit but cannot wait between contended attempts, is not a working service.
pub struct EntityServicePorts {
    /// Entity payload and snapshot reads.
    pub reader: Arc<dyn EntityReader>,
    /// Runtime-neutral execution of CPU-bound policy work.
    pub blocking_executor: Arc<dyn BlockingExecutor>,
    /// Runtime-neutral waiting between contended write attempts.
    pub retry_delay: Arc<dyn RetryDelay>,
    /// Historical dataset identifiers needed by attribute deletion.
    pub attribute_history: Arc<dyn AttributeDatasetHistory>,
    /// Durable context commits.
    pub writes: Arc<dyn ContextCommitter>,
}
