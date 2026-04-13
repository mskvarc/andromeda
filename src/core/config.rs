use crate::core::{cli::Cli, error::AndromedaError};
use clap::{Args, Parser};
use confique::Config;
use std::{
    collections::HashMap,
    net::IpAddr,
    path::{Path, PathBuf},
};

#[derive(Config, Debug, Clone)]
#[config(layer_attr(derive(Args, Clone)))]
pub struct Andromeda {
    #[config(env = "ANDROMEDA_HOST", default = "0.0.0.0")]
    #[config(layer_attr(arg(short = 'H', long)))]
    pub host: IpAddr,

    #[config(env = "ANDROMEDA_PORT", default = 1056)]
    #[config(layer_attr(arg(short = 'p', long)))]
    pub port: u16,
}

#[derive(Config, Debug, Clone)]
#[config(layer_attr(derive(Args, Clone)))]
pub struct Database {
    #[config(env = "ANDROMEDA_DATABASE_URL", default = "postgres://postgres:postgres@localhost:5432/postgres")]
    #[config(layer_attr(arg(long = "database-url")))]
    pub url: String,

    #[config(env = "ANDROMEDA_DATABASE_MAX_CONNECTIONS", default = 5)]
    #[config(layer_attr(arg(long = "database-max-connections")))]
    pub max_connections: u32,
}

#[derive(Config, Debug, Clone)]
#[config(layer_attr(derive(Args)))]
pub struct JsonLdConfig {
    #[config(default = true)]
    #[config(layer_attr(arg(long = "jsonld-allow-remote-contexts")))]
    pub allow_remote_contexts: bool,

    #[config(default = 30)]
    #[config(layer_attr(arg(long = "jsonld-timeout-seconds")))]
    pub timeout_seconds: u64,

    #[config(default = 5)]
    #[config(layer_attr(arg(long = "jsonld-max-redirects")))]
    pub max_redirects: u32,

    #[config(default = 2_000_000)]
    #[config(layer_attr(arg(long = "jsonld-max-context-bytes")))]
    pub max_context_bytes: u64,

    #[config(default = 1024)]
    #[config(layer_attr(arg(skip)))]
    pub cache_capacity: u64,

    #[config(default = 3600)]
    #[config(layer_attr(arg(skip)))]
    pub cache_ttl_seconds: u64,

    #[config(layer_attr(arg(skip)))]
    pub preload_contexts: Option<HashMap<String, String>>,
}

#[derive(Config, Debug, Clone)]
#[config(layer_attr(derive(Args)))]
pub struct Configuration {
    #[config(nested)]
    #[config(layer_attr(clap(flatten)))]
    pub andromeda: Andromeda,
    #[config(nested)]
    #[config(layer_attr(clap(flatten)))]
    pub database: Database,

    #[config(nested)]
    #[config(layer_attr(clap(flatten)))]
    pub jsonld: JsonLdConfig,

    #[config(nested)]
    #[config(layer_attr(clap(flatten)))]
    pub temporal: TemporalConfig,

    #[config(nested)]
    #[config(layer_attr(clap(flatten)))]
    pub multitenancy: MultitenancyConfig,

    #[config(nested)]
    #[config(layer_attr(clap(flatten)))]
    pub info: InfoConfig,

    #[config(nested)]
    #[config(layer_attr(clap(flatten)))]
    pub performance: PerformanceConfig,

    #[config(nested)]
    #[config(layer_attr(clap(flatten)))]
    pub outbox: OutboxConfig,
}

#[derive(Config, Debug, Clone)]
#[config(layer_attr(derive(Args, Clone)))]
pub struct InfoConfig {
    #[config(default = "urn:ngsi-ld:ContextSourceIdentity:andromeda")]
    #[config(layer_attr(arg(long = "source-identity")))]
    pub source_identity: String,
}

#[derive(Config, Debug, Clone)]
#[config(layer_attr(derive(Args)))]
pub struct TemporalConfig {
    /// System default lastN cap (NGSI-LD 6.3.10).
    #[config(default = 19)]
    #[config(layer_attr(arg(long = "temporal-default-last-n")))]
    pub default_last_n: u32,

    /// Interval in seconds between expiration checks.
    #[config(default = 30)]
    #[config(layer_attr(arg(skip)))]
    pub expiration_check_interval_secs: u64,
}

#[derive(Config, Debug, Clone)]
#[config(layer_attr(derive(Args)))]
pub struct MultitenancyConfig {
    #[config(env = "ANDROMEDA_MULTITENANCY_ENABLED", default = true)]
    #[config(layer_attr(arg(skip)))]
    pub enabled: bool,

    #[config(env = "ANDROMEDA_MULTITENANCY_TENANT_DB_PREFIX", default = "andromeda_tenant")]
    #[config(layer_attr(arg(skip)))]
    pub tenant_db_prefix: String,

    /// Maximum number of connections in per-tenant database pools.
    ///
    /// Each tenant gets its own connection pool capped at this value,
    /// preventing a single tenant from exhausting system-wide connection
    /// limits. Defaults to 3.
    #[config(default = 3)]
    #[config(layer_attr(arg(skip)))]
    pub tenant_max_connections: u32,

    #[config(default = 900)]
    #[config(layer_attr(arg(skip)))]
    pub runtime_idle_ttl_seconds: u64,

    #[config(default = 64)]
    #[config(layer_attr(arg(skip)))]
    pub runtime_max_cached: usize,

    #[config(default = 30)]
    #[config(layer_attr(arg(skip)))]
    pub janitor_interval_seconds: u64,
}

#[derive(Config, Debug, Clone)]
#[config(layer_attr(derive(Args)))]
pub struct PerformanceConfig {
    /// Maximum concurrent entities processed in batch operations.
    /// If omitted, the runtime uses `min(database.max_connections, 25)`.
    #[config(layer_attr(arg(long = "batch-concurrency")))]
    pub batch_concurrency: Option<usize>,

    /// Batch size used for chunked processing of large entity operation payloads.
    #[config(default = 512)]
    #[config(layer_attr(arg(long = "batch-chunk-size")))]
    pub batch_chunk_size: usize,

    /// Entity-count threshold above which the runtime switches to the large-batch path.
    #[config(default = 512)]
    #[config(layer_attr(arg(long = "batch-large-threshold")))]
    pub batch_large_threshold: usize,

    /// Entity-count threshold above which expensive preparation stages are moved off async workers.
    #[config(default = 128)]
    #[config(layer_attr(arg(long = "batch-prepare-blocking-threshold")))]
    pub batch_prepare_blocking_threshold: usize,

    /// Queue capacity for deferred batch side-effect processing jobs.
    #[config(default = 8)]
    #[config(layer_attr(arg(long = "batch-side-effect-queue-capacity")))]
    pub batch_side_effect_queue_capacity: usize,

    /// If set, successful batch side effects for batches at or above this size
    /// are deferred to a background worker.
    #[config(layer_attr(arg(long = "batch-async-side-effect-threshold")))]
    pub batch_async_side_effect_threshold: Option<usize>,

    /// Maximum HTTP request body size in bytes.
    /// Defaults to 64 MiB. Set higher for very large batch operations.
    #[config(default = 67_108_864)]
    #[config(layer_attr(arg(long = "max-request-body-bytes")))]
    pub max_request_body_bytes: usize,
}

#[derive(Config, Debug, Clone)]
#[config(layer_attr(derive(Args)))]
pub struct OutboxConfig {
    /// Enable writing outbox rows during entity mutations.
    #[config(default = false)]
    #[config(layer_attr(arg(skip)))]
    pub enabled: bool,

    /// Maximum retry attempts before dead-lettering.
    #[config(default = 5)]
    #[config(layer_attr(arg(skip)))]
    pub max_attempts: i32,

    /// Base backoff in seconds for exponential retry.
    #[config(default = 5)]
    #[config(layer_attr(arg(skip)))]
    pub base_backoff_secs: u64,

    /// Poll interval in seconds for the outbox delivery worker.
    #[config(default = 1)]
    #[config(layer_attr(arg(skip)))]
    pub poll_interval_secs: u64,

    /// Number of entries to claim per poll cycle.
    #[config(default = 50)]
    #[config(layer_attr(arg(skip)))]
    pub batch_size: i32,

    /// Delete delivered entries older than this many seconds.
    #[config(default = 86400)]
    #[config(layer_attr(arg(skip)))]
    pub cleanup_after_secs: u64,
}

impl Default for OutboxConfig {
    fn default() -> OutboxConfig {
        OutboxConfig {
            enabled: false,
            max_attempts: 5,
            base_backoff_secs: 5,
            poll_interval_secs: 1,
            batch_size: 50,
            cleanup_after_secs: 86400,
        }
    }
}

impl Default for PerformanceConfig {
    fn default() -> PerformanceConfig {
        PerformanceConfig {
            batch_concurrency: None,
            batch_chunk_size: 512,
            batch_large_threshold: 512,
            batch_prepare_blocking_threshold: 128,
            batch_side_effect_queue_capacity: 8,
            batch_async_side_effect_threshold: None,
            max_request_body_bytes: 67_108_864,
        }
    }
}

impl Configuration {
    pub fn new() -> Result<Configuration, AndromedaError> {
        let cli = Cli::parse();
        let config_file = cli.config_file.clone();

        let mut config = Configuration::builder().preloaded(cli.config).env().file(config_file.clone()).load()?;
        normalize_preload_context_paths(&mut config, &config_file);

        Ok(config)
    }
}

fn normalize_preload_context_paths(config: &mut Configuration, config_file: &Path) {
    let Some(preloads) = config.jsonld.preload_contexts.as_mut() else {
        return;
    };

    let config_dir = config_file.parent().map_or_else(|| PathBuf::from("."), PathBuf::from);

    for path in preloads.values_mut() {
        let candidate = PathBuf::from(path.as_str());
        if candidate.is_absolute() || candidate.exists() {
            continue;
        }

        *path = config_dir.join(candidate).to_string_lossy().into_owned();
    }
}

impl Default for JsonLdConfig {
    fn default() -> Self {
        Self {
            allow_remote_contexts: true,
            timeout_seconds: 30,
            max_redirects: 5,
            max_context_bytes: 2_000_000,
            cache_capacity: 1024,
            cache_ttl_seconds: 3600,
            preload_contexts: None,
        }
    }
}

impl Default for MultitenancyConfig {
    fn default() -> MultitenancyConfig {
        MultitenancyConfig {
            enabled: true,
            tenant_db_prefix: "andromeda_tenant".to_string(),
            tenant_max_connections: 3,
            runtime_idle_ttl_seconds: 900,
            runtime_max_cached: 64,
            janitor_interval_seconds: 30,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use confique::Layer;
    use serial_test::serial;

    use super::*;

    fn test_configuration_with_preload(path: &str) -> Configuration {
        let mut preloads = HashMap::new();
        preloads.insert("https://example.com/context.jsonld".to_string(), path.to_string());
        Configuration {
            andromeda: Andromeda {
                host: "127.0.0.1".parse().unwrap(),
                port: 1056,
            },
            database: Database {
                url: "postgres://postgres:postgres@localhost:5432/postgres".to_string(),
                max_connections: 5,
            },
            jsonld: JsonLdConfig {
                preload_contexts: Some(preloads),
                ..JsonLdConfig::default()
            },
            temporal: TemporalConfig {
                default_last_n: 19,
                expiration_check_interval_secs: 30,
            },
            multitenancy: MultitenancyConfig::default(),
            info: InfoConfig {
                source_identity: "urn:ngsi-ld:ContextSourceIdentity:andromeda".to_string(),
            },
            performance: PerformanceConfig::default(),
            outbox: OutboxConfig::default(),
        }
    }

    #[test]
    fn normalize_preload_paths_against_config_file_dir() {
        let mut config = test_configuration_with_preload("contexts/test.jsonld");
        normalize_preload_context_paths(&mut config, &PathBuf::from("/tmp/andromeda/config/andromeda.toml"));
        let path = config
            .jsonld
            .preload_contexts
            .as_ref()
            .and_then(|m| m.get("https://example.com/context.jsonld"))
            .expect("preload path missing");
        pretty_assertions::assert_eq!(path, "/tmp/andromeda/config/contexts/test.jsonld");
    }

    #[test]
    fn normalize_preload_paths_keeps_absolute_path() {
        let mut config = test_configuration_with_preload("/opt/contexts/test.jsonld");
        normalize_preload_context_paths(&mut config, &PathBuf::from("/tmp/andromeda/config/andromeda.toml"));
        let path = config
            .jsonld
            .preload_contexts
            .as_ref()
            .and_then(|m| m.get("https://example.com/context.jsonld"))
            .expect("preload path missing");
        pretty_assertions::assert_eq!(path, "/opt/contexts/test.jsonld");
    }

    #[test]
    #[serial]
    fn env_overrides_default_host() {
        unsafe { std::env::set_var("ANDROMEDA_HOST", "192.168.1.1") };
        let config = Configuration::builder().env().load().expect("config should load");
        unsafe { std::env::remove_var("ANDROMEDA_HOST") };
        pretty_assertions::assert_eq!(config.andromeda.host, IpAddr::from([192, 168, 1, 1]));
    }

    #[test]
    #[serial]
    fn env_overrides_default_port() {
        unsafe { std::env::set_var("ANDROMEDA_PORT", "9999") };
        let config = Configuration::builder().env().load().expect("config should load");
        unsafe { std::env::remove_var("ANDROMEDA_PORT") };
        pretty_assertions::assert_eq!(config.andromeda.port, 9999);
    }

    #[test]
    #[serial]
    fn env_overrides_database_url() {
        unsafe { std::env::set_var("ANDROMEDA_DATABASE_URL", "postgres://envhost/envdb") };
        let config = Configuration::builder().env().load().expect("config should load");
        unsafe { std::env::remove_var("ANDROMEDA_DATABASE_URL") };
        pretty_assertions::assert_eq!(config.database.url, "postgres://envhost/envdb");
    }

    #[test]
    #[serial]
    fn env_overrides_database_max_connections() {
        unsafe { std::env::set_var("ANDROMEDA_DATABASE_MAX_CONNECTIONS", "20") };
        let config = Configuration::builder().env().load().expect("config should load");
        unsafe { std::env::remove_var("ANDROMEDA_DATABASE_MAX_CONNECTIONS") };
        pretty_assertions::assert_eq!(config.database.max_connections, 20);
    }

    #[test]
    #[serial]
    fn env_overrides_multitenancy_enabled() {
        unsafe { std::env::set_var("ANDROMEDA_MULTITENANCY_ENABLED", "false") };
        let config = Configuration::builder().env().load().expect("config should load");
        unsafe { std::env::remove_var("ANDROMEDA_MULTITENANCY_ENABLED") };
        assert!(!config.multitenancy.enabled);
    }

    #[test]
    #[serial]
    fn env_overrides_multitenancy_tenant_db_prefix() {
        unsafe { std::env::set_var("ANDROMEDA_MULTITENANCY_TENANT_DB_PREFIX", "custom_prefix") };
        let config = Configuration::builder().env().load().expect("config should load");
        unsafe { std::env::remove_var("ANDROMEDA_MULTITENANCY_TENANT_DB_PREFIX") };
        pretty_assertions::assert_eq!(config.multitenancy.tenant_db_prefix, "custom_prefix");
    }

    #[test]
    #[serial]
    fn non_env_fields_ignore_env_vars() {
        unsafe { std::env::set_var("ANDROMEDA_JSONLD_CACHE_CAPACITY", "9999") };
        let config = Configuration::builder().env().load().expect("config should load");
        unsafe { std::env::remove_var("ANDROMEDA_JSONLD_CACHE_CAPACITY") };
        pretty_assertions::assert_eq!(config.jsonld.cache_capacity, 1024);
    }

    #[test]
    #[serial]
    fn preloaded_cli_overrides_env() {
        unsafe { std::env::set_var("ANDROMEDA_PORT", "8888") };

        let mut layer = <Configuration as Config>::Layer::default_values();
        layer.andromeda.port = Some(7777);

        let config = Configuration::builder().preloaded(layer).env().load().expect("config should load");

        unsafe { std::env::remove_var("ANDROMEDA_PORT") };
        pretty_assertions::assert_eq!(config.andromeda.port, 7777);
    }

    #[test]
    fn outbox_config_default_values() {
        let config = OutboxConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.max_attempts, 5);
        assert_eq!(config.base_backoff_secs, 5);
        assert_eq!(config.poll_interval_secs, 1);
        assert_eq!(config.batch_size, 50);
        assert_eq!(config.cleanup_after_secs, 86400);
    }
}
