//! How much of a batch runs where.
//!
//! Small batches stay on the calling task. Large ones split their preparation
//! across a blocking pool for the synchronous fast path and a future pool for
//! the asynchronous one, because JSON-LD expansion is CPU-bound for a document
//! whose `@context` is already resolved and IO-bound for one that is not.

use std::num::NonZeroUsize;

/// Runtime tuning for batch entity execution paths.
#[derive(Clone, Debug)]
pub struct BatchExecutionConfig {
    /// Maximum number of concurrent entity preparations.
    pub concurrency: usize,
    /// Maximum number of chunks dispatched in parallel during large-batch processing.
    /// Sizes the fastpath preparation blocking-pool semaphore (CPU-bound work).
    pub chunk_concurrency: usize,
    /// Maximum number of write-batch chunks dispatched in parallel for
    /// create/upsert/update/merge/delete. Distinct from `chunk_concurrency`,
    /// which sizes fastpath preparation; write paths are DB-bound and prefer a
    /// lower ceiling to avoid row-lock contention on `entities` and
    /// `temporal_entity_attributes`.
    pub chunk_write_concurrency: usize,
    /// Maximum number of entities processed per chunk in large-batch mode.
    pub chunk_size: usize,
    /// Entity-count threshold that enables chunked large-batch processing.
    pub large_threshold: usize,
    /// Entity-count threshold that offloads preparation post-processing to blocking workers.
    pub prepare_blocking_threshold: usize,
    /// Minimum batch size for honouring `Prefer: respond-async`. When `None`,
    /// the async durable-queue path is honoured for any batch size whenever
    /// the client opts in via the header. When `Some(t)`, the async path is
    /// only taken for batches of size `>= t` (smaller batches stay inline
    /// regardless of the client preference).
    pub async_side_effect_threshold: Option<usize>,
}

/// Concurrency mode for a batch's preparation phase.
#[derive(Clone, Copy)]
pub(super) enum BatchExecution {
    /// Process everything inline in the current task.
    Inline,
    /// Offload preparation to dedicated pools.
    Offload {
        /// Maximum number of concurrent fastpath blocking tasks.
        fast_chunks: NonZeroUsize,
        /// Maximum number of concurrent slow-path async tasks.
        slow_concurrency: NonZeroUsize,
    },
}

/// Choose an execution mode for a batch of `len` entities.
pub(super) fn decide_batch_execution(
    len: usize,
    _fast_count: usize,
    _slow_count: usize,
    blocking_threshold: usize,
    fast_chunk_concurrency: usize,
    slow_concurrency: usize,
) -> BatchExecution {
    if len < blocking_threshold {
        return BatchExecution::Inline;
    }
    let fast_chunks = NonZeroUsize::new(fast_chunk_concurrency.max(1)).unwrap_or(NonZeroUsize::MIN);
    let slow_concurrency = NonZeroUsize::new(slow_concurrency.max(1)).unwrap_or(NonZeroUsize::MIN);
    BatchExecution::Offload { fast_chunks, slow_concurrency }
}

#[cfg(test)]
mod tests {
    use super::{BatchExecution, decide_batch_execution};

    #[test]
    fn decide_batch_execution_below_threshold_returns_inline() {
        let exec = decide_batch_execution(64, 64, 0, 128, 4, 8);
        assert!(matches!(exec, BatchExecution::Inline));
    }

    #[test]
    fn decide_batch_execution_above_threshold_returns_offload() {
        let exec = decide_batch_execution(1024, 1000, 24, 128, 4, 8);
        match exec {
            BatchExecution::Offload { fast_chunks, slow_concurrency } => {
                pretty_assertions::assert_eq!(fast_chunks.get(), 4);
                pretty_assertions::assert_eq!(slow_concurrency.get(), 8);
            }
            BatchExecution::Inline => panic!("expected Offload"),
        }
    }

    #[test]
    fn decide_batch_execution_clamps_zero_concurrency_to_one() {
        let exec = decide_batch_execution(1024, 1024, 0, 128, 0, 0);
        match exec {
            BatchExecution::Offload { fast_chunks, slow_concurrency } => {
                pretty_assertions::assert_eq!(fast_chunks.get(), 1);
                pretty_assertions::assert_eq!(slow_concurrency.get(), 1);
            }
            BatchExecution::Inline => panic!("expected Offload"),
        }
    }
}
