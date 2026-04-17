use crate::core::error::AndromedaError;
use confique::{Config, Layer};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    net::IpAddr,
    path::{Path, PathBuf},
};
use strum::{Display, EnumString};

pub const DEFAULT_CONFIG_PATH: &str = "config/andromeda.toml";
const REDACTED_SECRET: &str = "<redacted>";

#[derive(Debug, Clone, Copy, Default)]
pub struct ServerOverrides {
    pub host: Option<IpAddr>,
    pub port: Option<u16>,
}

#[derive(Config, Debug, Clone, Serialize)]
pub struct ServerConfig {
    #[config(env = "ANDROMEDA_SERVER_HOST", default = "0.0.0.0")]
    pub host: IpAddr,

    #[config(env = "ANDROMEDA_SERVER_PORT", default = 1056)]
    pub port: u16,
}

/// Shared on/off switch for HTTP middleware features.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, Display, EnumString, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase", ascii_case_insensitive)]
pub enum HttpFeatureMode {
    Disabled,
    #[default]
    Enabled,
}

/// CORS mode for the broker's HTTP edge.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, Display, EnumString, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase", ascii_case_insensitive)]
pub enum CorsMode {
    Disabled,
    #[default]
    Permissive,
}

/// HTTP middleware configuration applied to all broker routes.
#[derive(Config, Debug, Clone, Serialize)]
pub struct HttpConfig {
    /// Maximum wall-clock time for a single HTTP request before the broker
    /// returns `408 Request Timeout`.
    #[config(env = "ANDROMEDA_HTTP_REQUEST_TIMEOUT_SECS", default = 30)]
    pub request_timeout_secs: u64,

    /// Whether the broker compresses eligible HTTP responses.
    #[config(env = "ANDROMEDA_HTTP_RESPONSE_COMPRESSION", default = "enabled")]
    pub response_compression: HttpFeatureMode,

    /// Whether the broker accepts compressed HTTP request bodies and
    /// transparently decompresses them for handlers.
    #[config(env = "ANDROMEDA_HTTP_REQUEST_DECOMPRESSION", default = "enabled")]
    pub request_decompression: HttpFeatureMode,

    /// CORS policy for browser-facing clients.
    #[config(env = "ANDROMEDA_HTTP_CORS", default = "permissive")]
    pub cors: CorsMode,
}

#[derive(Config, Debug, Clone, Serialize)]
pub struct DatabaseConfig {
    #[config(env = "ANDROMEDA_DATABASE_URL", default = "postgres://postgres:postgres@localhost:5432/postgres")]
    pub url: String,

    #[config(env = "ANDROMEDA_DATABASE_MAX_CONNECTIONS", default = 5)]
    pub max_connections: u32,
}

#[derive(Config, Debug, Clone, Serialize)]
pub struct JsonLdConfig {
    #[config(env = "ANDROMEDA_JSONLD_ALLOW_REMOTE_CONTEXTS", default = true)]
    pub allow_remote_contexts: bool,

    #[config(env = "ANDROMEDA_JSONLD_TIMEOUT_SECONDS", default = 30)]
    pub timeout_seconds: u64,

    #[config(env = "ANDROMEDA_JSONLD_MAX_REDIRECTS", default = 5)]
    pub max_redirects: u32,

    #[config(env = "ANDROMEDA_JSONLD_MAX_CONTEXT_BYTES", default = 2_000_000)]
    pub max_context_bytes: u64,

    #[config(env = "ANDROMEDA_JSONLD_CACHE_CAPACITY", default = 1024)]
    pub cache_capacity: u64,

    #[config(env = "ANDROMEDA_JSONLD_CACHE_TTL_SECONDS", default = 3600)]
    pub cache_ttl_seconds: u64,

    #[config(env = "ANDROMEDA_JSONLD_PRELOAD_CONTEXTS", parse_env = parse_string_map_json, default = {})]
    pub preload_contexts: HashMap<String, String>,
}

#[derive(Config, Debug, Clone, Serialize)]
pub struct TemporalConfig {
    /// System default lastN cap (NGSI-LD 6.3.10).
    #[config(env = "ANDROMEDA_TEMPORAL_DEFAULT_LAST_N", default = 19)]
    pub default_last_n: u32,

    /// Interval in seconds between expiration checks.
    #[config(env = "ANDROMEDA_TEMPORAL_EXPIRATION_CHECK_INTERVAL_SECS", default = 30)]
    pub expiration_check_interval_secs: u64,
}

#[derive(Config, Debug, Clone, Serialize)]
pub struct TenancyConfig {
    #[config(env = "ANDROMEDA_TENANCY_ENABLED", default = true)]
    pub enabled: bool,

    #[config(env = "ANDROMEDA_TENANCY_TENANT_DB_PREFIX", default = "andromeda_tenant")]
    pub tenant_db_prefix: String,

    /// Maximum number of connections in per-tenant database pools.
    ///
    /// Each tenant gets its own connection pool capped at this value,
    /// preventing a single tenant from exhausting system-wide connection
    /// limits.
    #[config(env = "ANDROMEDA_TENANCY_TENANT_MAX_CONNECTIONS", default = 3)]
    pub tenant_max_connections: u32,

    #[config(env = "ANDROMEDA_TENANCY_RUNTIME_IDLE_TTL_SECONDS", default = 900)]
    pub runtime_idle_ttl_seconds: u64,

    #[config(env = "ANDROMEDA_TENANCY_RUNTIME_MAX_CACHED", default = 64)]
    pub runtime_max_cached: usize,

    #[config(env = "ANDROMEDA_TENANCY_JANITOR_INTERVAL_SECONDS", default = 30)]
    pub janitor_interval_seconds: u64,
}

#[derive(Config, Debug, Clone, Serialize)]
pub struct InfoConfig {
    #[config(env = "ANDROMEDA_INFO_SOURCE_IDENTITY", default = "urn:ngsi-ld:ContextSourceIdentity:andromeda")]
    pub source_identity: String,
}

#[derive(Config, Debug, Clone, Serialize)]
pub struct PerformanceConfig {
    /// Maximum concurrent entities processed in batch operations.
    /// If omitted, the runtime uses `min(database.max_connections, 25)`.
    #[config(env = "ANDROMEDA_PERFORMANCE_BATCH_CONCURRENCY")]
    pub batch_concurrency: Option<usize>,

    /// Batch size used for chunked processing of large entity operation payloads.
    #[config(env = "ANDROMEDA_PERFORMANCE_BATCH_CHUNK_SIZE", default = 512)]
    pub batch_chunk_size: usize,

    /// Entity-count threshold above which the runtime switches to the large-batch path.
    #[config(env = "ANDROMEDA_PERFORMANCE_BATCH_LARGE_THRESHOLD", default = 512)]
    pub batch_large_threshold: usize,

    /// Entity-count threshold above which expensive preparation stages are moved off async workers.
    #[config(env = "ANDROMEDA_PERFORMANCE_BATCH_PREPARE_BLOCKING_THRESHOLD", default = 128)]
    pub batch_prepare_blocking_threshold: usize,

    /// Queue capacity for deferred batch side-effect processing jobs.
    #[config(env = "ANDROMEDA_PERFORMANCE_BATCH_SIDE_EFFECT_QUEUE_CAPACITY", default = 8)]
    pub batch_side_effect_queue_capacity: usize,

    /// If set, successful batch side effects for batches at or above this size
    /// are deferred to a background worker.
    #[config(env = "ANDROMEDA_PERFORMANCE_BATCH_ASYNC_SIDE_EFFECT_THRESHOLD")]
    pub batch_async_side_effect_threshold: Option<usize>,

    /// Maximum HTTP request body size in bytes.
    /// Defaults to 64 MiB. Set higher for very large batch operations.
    #[config(env = "ANDROMEDA_PERFORMANCE_MAX_REQUEST_BODY_BYTES", default = 67_108_864)]
    pub max_request_body_bytes: usize,

    /// Maximum number of concurrent snapshot executions (NGSI-LD clause
    /// 5.16.1.4). Snapshot creation spawns a background task that runs the
    /// configured queries; without a bound, a burst of create requests could
    /// saturate the database pool.
    #[config(env = "ANDROMEDA_PERFORMANCE_SNAPSHOT_CONCURRENCY", default = 4)]
    pub snapshot_concurrency: usize,

    /// Maximum number of parallel context-source forwarding requests per
    /// NGSI-LD operation. Each matching CSR generates one outbound HTTP
    /// request; without parallelism, wall-time equals the sum of CSR
    /// latencies. The default is tuned to saturate a typical WAN link
    /// without overwhelming downstream CSRs.
    #[config(env = "ANDROMEDA_PERFORMANCE_FORWARD_CSR_CONCURRENCY", default = 16)]
    pub forward_csr_concurrency: usize,
}

#[derive(Config, Debug, Clone, Serialize)]
pub struct OutboxConfig {
    /// Enable writing outbox rows during entity mutations.
    #[config(env = "ANDROMEDA_OUTBOX_ENABLED", default = false)]
    pub enabled: bool,

    /// Maximum retry attempts before dead-lettering.
    #[config(env = "ANDROMEDA_OUTBOX_MAX_ATTEMPTS", default = 5)]
    pub max_attempts: i32,

    /// Base backoff in seconds for exponential retry.
    #[config(env = "ANDROMEDA_OUTBOX_BASE_BACKOFF_SECS", default = 5)]
    pub base_backoff_secs: u64,

    /// Poll interval in seconds for the outbox delivery worker.
    #[config(env = "ANDROMEDA_OUTBOX_POLL_INTERVAL_SECS", default = 1)]
    pub poll_interval_secs: u64,

    /// Number of entries to claim per poll cycle.
    #[config(env = "ANDROMEDA_OUTBOX_BATCH_SIZE", default = 50)]
    pub batch_size: i32,

    /// Delete delivered entries older than this many seconds.
    #[config(env = "ANDROMEDA_OUTBOX_CLEANUP_AFTER_SECS", default = 86400)]
    pub cleanup_after_secs: u64,
}

/// SSE (Server-Sent Events) notification transport configuration.
#[derive(Config, Debug, Clone, Serialize)]
pub struct SseConfig {
    /// Per-connection channel buffer size for SSE notification delivery.
    #[config(env = "ANDROMEDA_NOTIFICATIONS_SSE_CHANNEL_BUFFER_SIZE", default = 64)]
    pub channel_buffer_size: usize,

    /// Keep-alive ping interval in seconds for idle SSE connections.
    #[config(env = "ANDROMEDA_NOTIFICATIONS_SSE_KEEP_ALIVE_INTERVAL_SECS", default = 15)]
    pub keep_alive_interval_secs: u64,
}

/// WebSocket notification transport configuration.
#[derive(Config, Debug, Clone, Serialize)]
pub struct WsConfig {
    /// Per-connection channel buffer size for WebSocket notification delivery.
    #[config(env = "ANDROMEDA_NOTIFICATIONS_WS_CHANNEL_BUFFER_SIZE", default = 64)]
    pub channel_buffer_size: usize,

    /// Ping interval in seconds for idle WebSocket connections.
    #[config(env = "ANDROMEDA_NOTIFICATIONS_WS_PING_INTERVAL_SECS", default = 30)]
    pub ping_interval_secs: u64,
}

#[derive(Config, Debug, Clone, Serialize)]
pub struct NotificationConfig {
    #[config(nested)]
    pub sse: SseConfig,

    #[config(nested)]
    pub ws: WsConfig,
}

/// NGSI-LD Data Integrity (ETSI GS CIM 019) configuration. The
/// `verification_methods` map binds verification-method URIs to
/// multibase-encoded public keys used by the static resolver. When
/// empty, only the HTTPS resolver applies and `verify=true` requests
/// dereference the verification method URL to fetch the key at runtime.
#[derive(Config, Debug, Clone, Serialize)]
pub struct ProvenanceConfig {
    #[config(env = "ANDROMEDA_PROVENANCE_VERIFICATION_METHODS", parse_env = parse_string_map_json, default = {})]
    pub verification_methods: HashMap<String, String>,

    /// When true, unknown verification-method URIs are dereferenced
    /// over HTTPS to fetch the `publicKeyMultibase` of a DID-document
    /// or JWK-style response. Off by default.
    #[config(env = "ANDROMEDA_PROVENANCE_HTTPS_RESOLVER_ENABLED", default = false)]
    pub https_resolver_enabled: bool,
}

#[derive(Config, Debug, Clone, Serialize)]
pub struct SpatialConfig {
    #[config(env = "ANDROMEDA_SPATIAL_REBUILD_INTERVAL_SECS", default = 1800)]
    pub rebuild_interval_secs: u64,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, Display, EnumString)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase", ascii_case_insensitive)]
pub enum LogLevel {
    Error,
    Warn,
    #[default]
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    pub const fn as_tracing_level(self) -> tracing::Level {
        match self {
            LogLevel::Error => tracing::Level::ERROR,
            LogLevel::Warn => tracing::Level::WARN,
            LogLevel::Info => tracing::Level::INFO,
            LogLevel::Debug => tracing::Level::DEBUG,
            LogLevel::Trace => tracing::Level::TRACE,
        }
    }
}

#[derive(Config, Debug, Clone, Serialize)]
pub struct ObservabilityConfig {
    #[config(env = "ANDROMEDA_OBSERVABILITY_LOG_LEVEL", default = "info")]
    pub log_level: LogLevel,
}

#[derive(Config, Debug, Clone, Serialize)]
pub struct RuntimeConfig {
    #[config(nested)]
    pub server: ServerConfig,

    #[config(nested)]
    pub http: HttpConfig,

    #[config(nested)]
    pub database: DatabaseConfig,

    #[config(nested)]
    pub jsonld: JsonLdConfig,

    #[config(nested)]
    pub temporal: TemporalConfig,

    #[config(nested)]
    pub tenancy: TenancyConfig,

    #[config(nested)]
    pub info: InfoConfig,

    #[config(nested)]
    pub performance: PerformanceConfig,

    #[config(nested)]
    pub outbox: OutboxConfig,

    #[config(nested)]
    pub notifications: NotificationConfig,

    #[config(nested)]
    pub provenance: ProvenanceConfig,

    #[config(nested)]
    pub spatial: SpatialConfig,

    #[config(nested)]
    pub observability: ObservabilityConfig,
}

impl RuntimeConfig {
    pub fn load(config_file: &Path, overrides: ServerOverrides) -> Result<RuntimeConfig, AndromedaError> {
        let mut cli_layer = <RuntimeConfig as Config>::Layer::empty();
        cli_layer.server.host = overrides.host;
        cli_layer.server.port = overrides.port;

        let mut config = RuntimeConfig::builder().preloaded(cli_layer).env().file(config_file).load()?;
        normalize_preload_context_paths(&mut config, config_file);
        validate_config(&config)?;

        Ok(config)
    }

    pub fn to_redacted_json(&self) -> Result<String, AndromedaError> {
        serde_json::to_string_pretty(&self.redacted())
            .map_err(|error| AndromedaError::internal(format!("failed to serialize effective config as JSON: {error}")))
    }

    pub fn to_redacted_toml(&self) -> Result<String, AndromedaError> {
        toml::to_string_pretty(&self.redacted()).map_err(|error| AndromedaError::internal(format!("failed to serialize effective config as TOML: {error}")))
    }

    pub const fn precedence_summary() -> &'static str {
        "CLI host/port > environment > config file > defaults"
    }

    fn redacted(&self) -> RuntimeConfig {
        // Clone into a printable snapshot so secrets can be redacted without mutating the live runtime config.
        let mut redacted = self.clone();
        redacted.database.url = REDACTED_SECRET.to_string();
        for value in redacted.provenance.verification_methods.values_mut() {
            *value = REDACTED_SECRET.to_string();
        }
        redacted
    }
}

fn parse_string_map_json(input: &str) -> Result<HashMap<String, String>, serde_json::Error> {
    serde_json::from_str(input)
}

fn validate_config(config: &RuntimeConfig) -> Result<(), AndromedaError> {
    if config.server.port == 0 {
        return Err(AndromedaError::internal("server.port must be greater than 0"));
    }
    if config.http.request_timeout_secs == 0 {
        return Err(AndromedaError::internal("http.request_timeout_secs must be at least 1"));
    }
    if config.database.url.is_empty() {
        return Err(AndromedaError::internal("database.url must not be empty"));
    }
    if config.database.max_connections == 0 {
        return Err(AndromedaError::internal("database.max_connections must be at least 1"));
    }
    if config.jsonld.timeout_seconds == 0 {
        return Err(AndromedaError::internal("jsonld.timeout_seconds must be at least 1"));
    }
    if config.jsonld.max_context_bytes == 0 {
        return Err(AndromedaError::internal("jsonld.max_context_bytes must be at least 1"));
    }
    if config.jsonld.cache_capacity == 0 {
        return Err(AndromedaError::internal("jsonld.cache_capacity must be at least 1"));
    }
    if config.jsonld.cache_ttl_seconds == 0 {
        return Err(AndromedaError::internal("jsonld.cache_ttl_seconds must be at least 1"));
    }
    if config.temporal.default_last_n == 0 {
        return Err(AndromedaError::internal("temporal.default_last_n must be at least 1"));
    }
    if config.temporal.expiration_check_interval_secs == 0 {
        return Err(AndromedaError::internal("temporal.expiration_check_interval_secs must be at least 1"));
    }
    if config.tenancy.tenant_db_prefix.is_empty() {
        return Err(AndromedaError::internal("tenancy.tenant_db_prefix must not be empty"));
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
    if config.info.source_identity.is_empty() {
        return Err(AndromedaError::internal("info.source_identity must not be empty"));
    }
    if let Some(batch_concurrency) = config.performance.batch_concurrency {
        if batch_concurrency == 0 {
            return Err(AndromedaError::internal("performance.batch_concurrency must be at least 1"));
        }
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
    if let Some(threshold) = config.performance.batch_async_side_effect_threshold {
        if threshold == 0 {
            return Err(AndromedaError::internal("performance.batch_async_side_effect_threshold must be at least 1"));
        }
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

    Ok(())
}

fn normalize_preload_context_paths(config: &mut RuntimeConfig, config_file: &Path) {
    if config.jsonld.preload_contexts.is_empty() {
        return;
    }

    let config_dir = config_file.parent().map_or_else(|| PathBuf::from("."), PathBuf::from);

    for path in config.jsonld.preload_contexts.values_mut() {
        let candidate = PathBuf::from(path.as_str());
        if candidate.is_absolute() || candidate.exists() {
            continue;
        }

        *path = config_dir.join(candidate).to_string_lossy().into_owned();
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use serial_test::serial;
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;

    fn test_runtime_config_with_preload(path: &str) -> RuntimeConfig {
        let mut preloads = HashMap::new();
        preloads.insert("https://example.com/context.jsonld".to_string(), path.to_string());
        RuntimeConfig {
            server: ServerConfig {
                host: "127.0.0.1".parse().unwrap(),
                port: 1056,
            },
            http: HttpConfig {
                request_timeout_secs: 30,
                response_compression: HttpFeatureMode::Enabled,
                request_decompression: HttpFeatureMode::Enabled,
                cors: CorsMode::Permissive,
            },
            database: DatabaseConfig {
                url: "postgres://postgres:postgres@localhost:5432/postgres".to_string(),
                max_connections: 5,
            },
            jsonld: JsonLdConfig {
                allow_remote_contexts: true,
                timeout_seconds: 30,
                max_redirects: 5,
                max_context_bytes: 2_000_000,
                cache_capacity: 1024,
                cache_ttl_seconds: 3600,
                preload_contexts: preloads,
            },
            temporal: TemporalConfig {
                default_last_n: 19,
                expiration_check_interval_secs: 30,
            },
            tenancy: TenancyConfig {
                enabled: true,
                tenant_db_prefix: "andromeda_tenant".to_string(),
                tenant_max_connections: 3,
                runtime_idle_ttl_seconds: 900,
                runtime_max_cached: 64,
                janitor_interval_seconds: 30,
            },
            info: InfoConfig {
                source_identity: "urn:ngsi-ld:ContextSourceIdentity:andromeda".to_string(),
            },
            performance: PerformanceConfig {
                batch_concurrency: None,
                batch_chunk_size: 512,
                batch_large_threshold: 512,
                batch_prepare_blocking_threshold: 128,
                batch_side_effect_queue_capacity: 8,
                batch_async_side_effect_threshold: None,
                max_request_body_bytes: 67_108_864,
                snapshot_concurrency: 4,
                forward_csr_concurrency: 16,
            },
            outbox: OutboxConfig {
                enabled: false,
                max_attempts: 5,
                base_backoff_secs: 5,
                poll_interval_secs: 1,
                batch_size: 50,
                cleanup_after_secs: 86400,
            },
            notifications: NotificationConfig {
                sse: SseConfig {
                    channel_buffer_size: 64,
                    keep_alive_interval_secs: 15,
                },
                ws: WsConfig {
                    channel_buffer_size: 64,
                    ping_interval_secs: 30,
                },
            },
            provenance: ProvenanceConfig {
                verification_methods: HashMap::new(),
                https_resolver_enabled: false,
            },
            spatial: SpatialConfig { rebuild_interval_secs: 1800 },
            observability: ObservabilityConfig { log_level: LogLevel::Info },
        }
    }

    fn write_temp_config(contents: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("andromeda-config-{unique}.toml"));
        fs::write(&path, contents).expect("temp config should be written");
        path
    }

    #[test]
    fn normalize_preload_paths_against_config_file_dir() {
        let mut config = test_runtime_config_with_preload("contexts/test.jsonld");
        normalize_preload_context_paths(&mut config, &PathBuf::from("/tmp/andromeda/config/andromeda.toml"));
        let path = config
            .jsonld
            .preload_contexts
            .get("https://example.com/context.jsonld")
            .expect("preload path missing");
        pretty_assertions::assert_eq!(path, "/tmp/andromeda/config/contexts/test.jsonld");
    }

    #[test]
    fn normalize_preload_paths_keeps_absolute_path() {
        let mut config = test_runtime_config_with_preload("/opt/contexts/test.jsonld");
        normalize_preload_context_paths(&mut config, &PathBuf::from("/tmp/andromeda/config/andromeda.toml"));
        let path = config
            .jsonld
            .preload_contexts
            .get("https://example.com/context.jsonld")
            .expect("preload path missing");
        pretty_assertions::assert_eq!(path, "/opt/contexts/test.jsonld");
    }

    #[test]
    #[serial]
    fn env_overrides_default_host() {
        unsafe { std::env::set_var("ANDROMEDA_SERVER_HOST", "192.168.1.1") };
        let config = RuntimeConfig::builder().env().load().expect("config should load");
        unsafe { std::env::remove_var("ANDROMEDA_SERVER_HOST") };
        pretty_assertions::assert_eq!(config.server.host, IpAddr::from([192, 168, 1, 1]));
    }

    #[test]
    #[serial]
    fn env_overrides_default_port() {
        unsafe { std::env::set_var("ANDROMEDA_SERVER_PORT", "9999") };
        let config = RuntimeConfig::builder().env().load().expect("config should load");
        unsafe { std::env::remove_var("ANDROMEDA_SERVER_PORT") };
        pretty_assertions::assert_eq!(config.server.port, 9999);
    }

    #[test]
    #[serial]
    fn env_overrides_database_url() {
        unsafe { std::env::set_var("ANDROMEDA_DATABASE_URL", "postgres://envhost/envdb") };
        let config = RuntimeConfig::builder().env().load().expect("config should load");
        unsafe { std::env::remove_var("ANDROMEDA_DATABASE_URL") };
        pretty_assertions::assert_eq!(config.database.url, "postgres://envhost/envdb");
    }

    #[test]
    #[serial]
    fn env_overrides_jsonld_cache_capacity() {
        unsafe { std::env::set_var("ANDROMEDA_JSONLD_CACHE_CAPACITY", "9999") };
        let config = RuntimeConfig::builder().env().load().expect("config should load");
        unsafe { std::env::remove_var("ANDROMEDA_JSONLD_CACHE_CAPACITY") };
        pretty_assertions::assert_eq!(config.jsonld.cache_capacity, 9999);
    }

    #[test]
    #[serial]
    fn env_parses_http_response_compression_mode() {
        unsafe { std::env::set_var("ANDROMEDA_HTTP_RESPONSE_COMPRESSION", "disabled") };
        let config = RuntimeConfig::builder().env().load().expect("config should load");
        unsafe { std::env::remove_var("ANDROMEDA_HTTP_RESPONSE_COMPRESSION") };
        pretty_assertions::assert_eq!(config.http.response_compression, HttpFeatureMode::Disabled);
    }

    #[test]
    #[serial]
    fn env_parses_http_cors_mode() {
        unsafe { std::env::set_var("ANDROMEDA_HTTP_CORS", "disabled") };
        let config = RuntimeConfig::builder().env().load().expect("config should load");
        unsafe { std::env::remove_var("ANDROMEDA_HTTP_CORS") };
        pretty_assertions::assert_eq!(config.http.cors, CorsMode::Disabled);
    }

    #[test]
    #[serial]
    fn env_parses_json_maps_for_hashmap_fields() {
        unsafe {
            std::env::set_var(
                "ANDROMEDA_JSONLD_PRELOAD_CONTEXTS",
                "{\"https://example.com/context.jsonld\":\"contexts/test.jsonld\"}",
            )
        };
        let config = RuntimeConfig::builder().env().load().expect("config should load");
        unsafe { std::env::remove_var("ANDROMEDA_JSONLD_PRELOAD_CONTEXTS") };
        pretty_assertions::assert_eq!(
            config.jsonld.preload_contexts.get("https://example.com/context.jsonld"),
            Some(&"contexts/test.jsonld".to_string())
        );
    }

    #[test]
    #[serial]
    fn file_overrides_defaults() {
        let path = write_temp_config(
            r#"
[server]
port = 1600

[notifications.ws]
ping_interval_secs = 45
"#,
        );

        let config = RuntimeConfig::load(&path, ServerOverrides::default()).expect("config should load");
        fs::remove_file(path).expect("temp config should be removed");

        pretty_assertions::assert_eq!(config.server.port, 1600);
        pretty_assertions::assert_eq!(config.notifications.ws.ping_interval_secs, 45);
    }

    #[test]
    #[serial]
    fn env_overrides_file() {
        let path = write_temp_config(
            r#"
[server]
port = 1600
"#,
        );

        unsafe { std::env::set_var("ANDROMEDA_SERVER_PORT", "1700") };
        let config = RuntimeConfig::load(&path, ServerOverrides::default()).expect("config should load");
        unsafe { std::env::remove_var("ANDROMEDA_SERVER_PORT") };
        fs::remove_file(path).expect("temp config should be removed");

        pretty_assertions::assert_eq!(config.server.port, 1700);
    }

    #[test]
    #[serial]
    fn cli_overrides_env() {
        unsafe { std::env::set_var("ANDROMEDA_SERVER_PORT", "8888") };
        let config = RuntimeConfig::load(Path::new(DEFAULT_CONFIG_PATH), ServerOverrides { host: None, port: Some(7777) }).expect("config should load");
        unsafe { std::env::remove_var("ANDROMEDA_SERVER_PORT") };

        pretty_assertions::assert_eq!(config.server.port, 7777);
    }

    #[test]
    #[serial]
    fn legacy_env_names_are_ignored() {
        unsafe { std::env::set_var("ANDROMEDA_PORT", "8888") };
        let config = RuntimeConfig::builder().env().load().expect("config should load");
        unsafe { std::env::remove_var("ANDROMEDA_PORT") };

        pretty_assertions::assert_eq!(config.server.port, 1056);
    }

    #[test]
    fn redacted_effective_config_hides_secrets() {
        let mut config = test_runtime_config_with_preload("contexts/test.jsonld");
        config.database.url = "postgres://secret".to_string();
        config
            .provenance
            .verification_methods
            .insert("https://example.com/verification-method".to_string(), "zSecretMultibase".to_string());

        let rendered = config.to_redacted_toml().expect("redacted toml should render");

        assert!(!rendered.contains("postgres://secret"));
        assert!(!rendered.contains("zSecretMultibase"));
        assert!(rendered.contains(REDACTED_SECRET));
    }

    #[test]
    fn validate_rejects_zero_spatial_rebuild_interval() {
        let mut config = test_runtime_config_with_preload("contexts/test.jsonld");
        config.spatial.rebuild_interval_secs = 0;

        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn validate_rejects_zero_http_request_timeout() {
        let mut config = test_runtime_config_with_preload("contexts/test.jsonld");
        config.http.request_timeout_secs = 0;

        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn default_precedence_summary_matches_contract() {
        pretty_assertions::assert_eq!(RuntimeConfig::precedence_summary(), "CLI host/port > environment > config file > defaults");
    }

    #[test]
    fn config_layer_default_is_empty() {
        let layer = <RuntimeConfig as Config>::Layer::empty();
        assert!(layer.server.host.is_none());
        assert!(layer.server.port.is_none());
    }
}
