//! Broker concurrency and batching configuration.

use confique::Config;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// Bulk-write execution mode for batch entity writes.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, Display, EnumString, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
pub enum BulkWriteMode {
    /// Send PostgreSQL arrays through `UNNEST`.
    #[default]
    Unnest,
    /// Stream rows into a transaction-scoped staging table.
    CopyStaging,
}

/// Performance-related limits and worker cadence.
#[derive(Config, Debug, Clone, Serialize)]
pub struct PerformanceConfig {
    /// Maximum concurrent entities processed in batch operations.
    #[config(env = "ANDROMEDA_PERFORMANCE_BATCH_CONCURRENCY")]
    pub batch_concurrency: Option<usize>,
    /// Maximum concurrent CPU-bound preparation chunks.
    #[config(env = "ANDROMEDA_PERFORMANCE_BATCH_CHUNK_CONCURRENCY")]
    pub batch_chunk_concurrency: Option<usize>,
    /// Maximum concurrent database write chunks.
    #[config(env = "ANDROMEDA_PERFORMANCE_BATCH_CHUNK_WRITE_CONCURRENCY")]
    pub batch_chunk_write_concurrency: Option<usize>,
    /// Populate the attribute-name cache at tenant startup.
    #[config(env = "ANDROMEDA_PERFORMANCE_PRELOAD_ATTRIBUTE_NAMES", default = true)]
    pub preload_attribute_names: bool,
    /// Number of entities processed in one batch chunk.
    #[config(env = "ANDROMEDA_PERFORMANCE_BATCH_CHUNK_SIZE", default = 512)]
    pub batch_chunk_size: usize,
    /// Entity threshold selecting the large-batch path.
    #[config(env = "ANDROMEDA_PERFORMANCE_BATCH_LARGE_THRESHOLD", default = 512)]
    pub batch_large_threshold: usize,
    /// Entity threshold selecting blocking-pool preparation.
    #[config(env = "ANDROMEDA_PERFORMANCE_BATCH_PREPARE_BLOCKING_THRESHOLD", default = 128)]
    pub batch_prepare_blocking_threshold: usize,
    /// Capacity retained for the former in-memory side-effect dispatcher.
    #[config(env = "ANDROMEDA_PERFORMANCE_BATCH_SIDE_EFFECT_QUEUE_CAPACITY", default = 8)]
    pub batch_side_effect_queue_capacity: usize,
    /// Minimum batch size eligible for asynchronous side effects.
    #[config(env = "ANDROMEDA_PERFORMANCE_BATCH_ASYNC_SIDE_EFFECT_THRESHOLD")]
    pub batch_async_side_effect_threshold: Option<usize>,
    /// Maximum entity or attribute-expiry candidates read per sweep phase.
    #[config(env = "ANDROMEDA_PERFORMANCE_ENTITY_EXPIRY_BATCH_SIZE", default = 1_000)]
    pub entity_expiry_batch_size: usize,
    /// Poll interval for durable pending side effects.
    #[config(env = "ANDROMEDA_PERFORMANCE_PENDING_SIDE_EFFECTS_POLL_SECS", default = 1)]
    pub pending_side_effects_poll_secs: u64,
    /// Lease duration for a claimed side-effect job.
    #[config(env = "ANDROMEDA_PERFORMANCE_PENDING_SIDE_EFFECTS_LEASE_SECS", default = 30)]
    pub pending_side_effects_lease_secs: u64,
    /// Maximum jobs claimed per poll.
    #[config(env = "ANDROMEDA_PERFORMANCE_PENDING_SIDE_EFFECTS_BATCH_SIZE", default = 32)]
    pub pending_side_effects_batch_size: usize,
    /// Attempts before a side-effect job is dead-lettered.
    #[config(env = "ANDROMEDA_PERFORMANCE_PENDING_SIDE_EFFECTS_MAX_ATTEMPTS", default = 5)]
    pub pending_side_effects_max_attempts: u16,
    /// Base retry backoff for side-effect jobs.
    #[config(env = "ANDROMEDA_PERFORMANCE_PENDING_SIDE_EFFECTS_BASE_BACKOFF_SECS", default = 2)]
    pub pending_side_effects_base_backoff_secs: u64,
    /// Database bulk-write strategy.
    #[config(env = "ANDROMEDA_PERFORMANCE_BULK_WRITE_MODE", default = "unnest")]
    pub bulk_write_mode: BulkWriteMode,
    /// Maximum accepted HTTP request body size.
    #[config(env = "ANDROMEDA_PERFORMANCE_MAX_REQUEST_BODY_BYTES", default = 67_108_864)]
    pub max_request_body_bytes: usize,
    /// Maximum concurrent snapshot executions.
    #[config(env = "ANDROMEDA_PERFORMANCE_SNAPSHOT_CONCURRENCY", default = 4)]
    pub snapshot_concurrency: usize,
    /// Maximum parallel Context Source forwarding requests.
    #[config(env = "ANDROMEDA_PERFORMANCE_FORWARD_CSR_CONCURRENCY", default = 16)]
    pub forward_csr_concurrency: usize,
    /// Interval between JSON-LD hit-counter flushes.
    #[config(env = "ANDROMEDA_PERFORMANCE_CONTEXT_HIT_FLUSH_SECS", default = 5)]
    pub context_hit_flush_secs: u64,
}
