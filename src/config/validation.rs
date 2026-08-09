//! Runtime configuration validation and path normalization.

use crate::{config::runtime::RuntimeConfig, error::AndromedaError};
use std::path::{Path, PathBuf};

pub(super) fn validate_config(config: &RuntimeConfig) -> Result<(), AndromedaError> {
    if config.server.port == 0 {
        return Err(AndromedaError::internal("server.port must be greater than 0"));
    }
    if config.http.request_timeout_secs == 0 {
        return Err(AndromedaError::internal("http.request_timeout_secs must be at least 1"));
    }
    if config.database.max_connections == 0 {
        return Err(AndromedaError::internal("database.max_connections must be at least 1"));
    }
    config.jsonld.validate().map_err(|error| AndromedaError::internal(error.to_string()))?;
    if config.temporal.default_last_n == 0 {
        return Err(AndromedaError::internal("temporal.default_last_n must be at least 1"));
    }
    if config.temporal.expiration_check_interval_secs == 0 {
        return Err(AndromedaError::internal("temporal.expiration_check_interval_secs must be at least 1"));
    }
    if config.tenancy.tenant_max_connections == 0 {
        return Err(AndromedaError::internal("tenancy.tenant_max_connections must be at least 1"));
    }
    if config.tenancy.runtime_idle_ttl_seconds == 0 {
        return Err(AndromedaError::internal("tenancy.runtime_idle_ttl_seconds must be at least 1"));
    }
    if config.tenancy.runtime_max_cached == 0 {
        return Err(AndromedaError::internal("tenancy.runtime_max_cached must be at least 1"));
    }
    if config.tenancy.janitor_interval_seconds == 0 {
        return Err(AndromedaError::internal("tenancy.janitor_interval_seconds must be at least 1"));
    }
    if let Some(batch_concurrency) = config.performance.batch_concurrency
        && batch_concurrency == 0
    {
        return Err(AndromedaError::internal("performance.batch_concurrency must be at least 1"));
    }
    if let Some(batch_chunk_concurrency) = config.performance.batch_chunk_concurrency
        && batch_chunk_concurrency == 0
    {
        return Err(AndromedaError::internal("performance.batch_chunk_concurrency must be at least 1"));
    }
    if let Some(batch_chunk_write_concurrency) = config.performance.batch_chunk_write_concurrency
        && batch_chunk_write_concurrency == 0
    {
        return Err(AndromedaError::internal("performance.batch_chunk_write_concurrency must be at least 1"));
    }
    if config.performance.batch_chunk_size == 0 {
        return Err(AndromedaError::internal("performance.batch_chunk_size must be at least 1"));
    }
    if config.performance.batch_large_threshold == 0 {
        return Err(AndromedaError::internal("performance.batch_large_threshold must be at least 1"));
    }
    if config.performance.batch_prepare_blocking_threshold == 0 {
        return Err(AndromedaError::internal("performance.batch_prepare_blocking_threshold must be at least 1"));
    }
    if config.performance.batch_side_effect_queue_capacity == 0 {
        return Err(AndromedaError::internal("performance.batch_side_effect_queue_capacity must be at least 1"));
    }
    if let Some(threshold) = config.performance.batch_async_side_effect_threshold
        && threshold == 0
    {
        return Err(AndromedaError::internal("performance.batch_async_side_effect_threshold must be at least 1"));
    }
    if config.performance.pending_side_effects_poll_secs == 0 {
        return Err(AndromedaError::internal("performance.pending_side_effects_poll_secs must be at least 1"));
    }
    if config.performance.pending_side_effects_lease_secs == 0 {
        return Err(AndromedaError::internal("performance.pending_side_effects_lease_secs must be at least 1"));
    }
    if config.performance.pending_side_effects_batch_size == 0 {
        return Err(AndromedaError::internal("performance.pending_side_effects_batch_size must be at least 1"));
    }
    if config.performance.pending_side_effects_max_attempts == 0 {
        return Err(AndromedaError::internal("performance.pending_side_effects_max_attempts must be at least 1"));
    }
    if config.performance.pending_side_effects_base_backoff_secs == 0 {
        return Err(AndromedaError::internal(
            "performance.pending_side_effects_base_backoff_secs must be at least 1",
        ));
    }
    if config.performance.max_request_body_bytes == 0 {
        return Err(AndromedaError::internal("performance.max_request_body_bytes must be at least 1"));
    }
    if config.performance.snapshot_concurrency == 0 {
        return Err(AndromedaError::internal("performance.snapshot_concurrency must be at least 1"));
    }
    if config.performance.forward_csr_concurrency == 0 {
        return Err(AndromedaError::internal("performance.forward_csr_concurrency must be at least 1"));
    }
    if config.performance.context_hit_flush_secs == 0 {
        return Err(AndromedaError::internal("performance.context_hit_flush_secs must be at least 1"));
    }
    if config.outbox.max_attempts <= 0 {
        return Err(AndromedaError::internal("outbox.max_attempts must be at least 1"));
    }
    if config.outbox.base_backoff_secs == 0 {
        return Err(AndromedaError::internal("outbox.base_backoff_secs must be at least 1"));
    }
    if config.outbox.poll_interval_secs == 0 {
        return Err(AndromedaError::internal("outbox.poll_interval_secs must be at least 1"));
    }
    if config.outbox.batch_size <= 0 {
        return Err(AndromedaError::internal("outbox.batch_size must be at least 1"));
    }
    if config.outbox.cleanup_after_secs == 0 {
        return Err(AndromedaError::internal("outbox.cleanup_after_secs must be at least 1"));
    }
    if config.notifications.sse.channel_buffer_size == 0 {
        return Err(AndromedaError::internal("notifications.sse.channel_buffer_size must be at least 1"));
    }
    if config.notifications.sse.keep_alive_interval_secs == 0 {
        return Err(AndromedaError::internal("notifications.sse.keep_alive_interval_secs must be at least 1"));
    }
    if config.notifications.ws.channel_buffer_size == 0 {
        return Err(AndromedaError::internal("notifications.ws.channel_buffer_size must be at least 1"));
    }
    if config.notifications.ws.ping_interval_secs == 0 {
        return Err(AndromedaError::internal("notifications.ws.ping_interval_secs must be at least 1"));
    }
    if config.spatial.rebuild_interval_secs == 0 {
        return Err(AndromedaError::internal("spatial.rebuild_interval_secs must be at least 1"));
    }
    if config.spatial.change_channel_capacity == 0 {
        return Err(AndromedaError::internal("spatial.change_channel_capacity must be at least 1"));
    }
    if config.spatial.lag_log_interval_secs == 0 {
        return Err(AndromedaError::internal("spatial.lag_log_interval_secs must be at least 1"));
    }

    Ok(())
}

pub(super) fn normalize_preload_context_paths(config: &mut RuntimeConfig, config_file: &Path) {
    let config_dir = config_file.parent().map_or_else(|| PathBuf::from("."), PathBuf::from);
    config.jsonld.resolve_preload_paths(&config_dir);
}
