//! How much of a batch runs where.
//!
//! Small batches stay on the calling task. Large ones spread the typed
//! expansion of their preparation across a blocking pool, because expanding a
//! document whose `@context` is already processed is CPU-bound; an entity the
//! typed walk declines is expanded by the JSON-LD processor on the calling
//! task.

use std::num::NonZeroUsize;

/// Runtime tuning for batch entity execution paths.
#[derive(Clone, Debug)]
pub struct BatchExecutionConfig {
    /// Maximum number of chunks dispatched in parallel during large-batch processing.
    /// Sizes how many typed-expansion tasks run on the blocking pool at once
    /// (CPU-bound work).
    pub chunk_concurrency: usize,
    /// Maximum number of write-batch chunks dispatched in parallel for
    /// create/upsert/update/merge/delete. Distinct from `chunk_concurrency`,
    /// which sizes the preparation; write paths are DB-bound and prefer a
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

impl BatchExecutionConfig {
    /// The same tuning with every limit raised to at least one.
    ///
    /// The batch paths chunk and stream by these limits, so a configured zero
    /// would produce an empty chunk or a stream that never polls; the entity
    /// policy clamps once, when it takes the configuration.
    #[must_use]
    pub(crate) fn clamped_to_usable_limits(self) -> BatchExecutionConfig {
        BatchExecutionConfig {
            chunk_concurrency: self.chunk_concurrency.max(1),
            chunk_write_concurrency: self.chunk_write_concurrency.max(1),
            chunk_size: self.chunk_size.max(1),
            large_threshold: self.large_threshold.max(1),
            prepare_blocking_threshold: self.prepare_blocking_threshold.max(1),
            async_side_effect_threshold: self.async_side_effect_threshold,
        }
    }
}

/// Concurrency mode for a batch's preparation phase.
#[derive(Clone, Copy)]
pub(super) enum BatchExecution {
    /// Process everything inline in the current task.
    Inline,
    /// Offload the typed walk of the preparation to the blocking pool.
    Offload {
        /// Maximum number of walk tasks running at once.
        concurrency: NonZeroUsize,
    },
}

/// Choose an execution mode for a batch of `len` entities.
pub(super) fn decide_batch_execution(len: usize, blocking_threshold: usize, walk_concurrency: usize) -> BatchExecution {
    if len < blocking_threshold {
        return BatchExecution::Inline;
    }
    let concurrency = NonZeroUsize::new(walk_concurrency.max(1)).unwrap_or(NonZeroUsize::MIN);
    BatchExecution::Offload { concurrency }
}

#[cfg(test)]
mod tests {
    use crate::batch::execution::{BatchExecution, decide_batch_execution};

    #[test]
    fn decide_batch_execution_below_threshold_returns_inline() {
        let exec = decide_batch_execution(64, 128, 4);
        assert!(matches!(exec, BatchExecution::Inline));
    }

    #[test]
    fn decide_batch_execution_above_threshold_returns_offload() {
        let exec = decide_batch_execution(1024, 128, 4);
        assert!(matches!(exec, BatchExecution::Offload { concurrency } if concurrency.get() == 4));
    }

    #[test]
    fn decide_batch_execution_clamps_zero_concurrency_to_one() {
        let exec = decide_batch_execution(1024, 128, 0);
        assert!(matches!(exec, BatchExecution::Offload { concurrency } if concurrency.get() == 1));
    }
}
