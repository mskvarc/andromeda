use crate::{
    config::{
        error::ConfigurationError,
        observability::ObservabilityConfig,
        validation::{normalize_preload_context_paths, validate_config},
    },
    outcome::ProcessExitOutcome,
};
use andromeda_entity::{config::EntityConfig, spatial_config::SpatialConfig};
use andromeda_federation::config::FederationConfig;
use andromeda_http_server::config::{ServerConfig, ServerOverrides};
use andromeda_http_source_identity::config::InfoConfig;
use andromeda_http_stack::config::HttpStackConfig;
use andromeda_http_subscription::config::NotificationConfig;
use andromeda_jsonld::config::JsonLdConfig;
use andromeda_notification::delivery_config::DeliveryConfig;
use andromeda_postgresql_atomic_write::config::AtomicWriteConfig;
use andromeda_postgresql_context_store::config::DatabaseConfig;
use andromeda_snapshot::config::SnapshotConfig;
use andromeda_temporal::config::TemporalConfig;
use andromeda_tenancy::config::TenancyConfig;
use confique::{Config, Layer};
use serde::Serialize;
use std::{net::IpAddr, path::Path};
use url::Url;

/// Default broker configuration path used by the CLI.
pub const DEFAULT_CONFIG_PATH: &str = "config/andromeda.toml";

/// Fully loaded broker construction configuration.
#[derive(Config, Debug, Clone, Serialize)]
pub struct RuntimeConfig {
    /// HTTP listener and graceful-drain configuration.
    #[config(nested)]
    pub server: ServerConfig,

    /// Process-wide inbound HTTP stack configuration.
    #[config(nested)]
    pub http: HttpStackConfig,

    /// Primary and tenant PostgreSQL connection configuration.
    #[config(nested)]
    pub database: DatabaseConfig,

    /// JSON-LD processing and context-loading configuration.
    #[config(nested)]
    pub jsonld: JsonLdConfig,

    /// Temporal policy and append-mode configuration.
    #[config(nested)]
    pub temporal: TemporalConfig,

    /// Tenant creation and runtime-cache configuration.
    #[config(nested)]
    pub tenancy: TenancyConfig,

    /// Context Source identity configuration.
    #[config(nested)]
    pub info: InfoConfig,

    /// Entity batching, expiry, and durable side-effect configuration.
    #[config(nested)]
    pub entity: EntityConfig,

    /// Distributed Context Source forwarding configuration.
    #[config(nested)]
    pub federation: FederationConfig,

    /// PostgreSQL atomic-write strategy.
    #[config(nested)]
    pub postgresql_write: AtomicWriteConfig,

    /// Snapshot operation configuration.
    #[config(nested)]
    pub snapshot: SnapshotConfig,

    /// Durable notification event and delivery configuration.
    #[config(nested)]
    pub notification_delivery: DeliveryConfig,

    /// Streaming notification transport configuration.
    #[config(nested)]
    pub notifications: NotificationConfig,

    /// Spatial projection and reconciliation configuration.
    #[config(nested)]
    pub spatial: SpatialConfig,

    /// Composition-owned tracing configuration.
    #[config(nested)]
    pub observability: ObservabilityConfig,
}

impl RuntimeConfig {
    /// Load, normalize, and validate the configured broker resources.
    fn load(config_file: &Path, overrides: ServerOverrides) -> Result<RuntimeConfig, ConfigurationError> {
        let mut cli_layer = <RuntimeConfig as Config>::Layer::empty();
        cli_layer.server.host = overrides.host;
        cli_layer.server.port = overrides.port;

        let mut config = RuntimeConfig::builder().preloaded(cli_layer).env().file(config_file).load()?;
        normalize_preload_context_paths(&mut config, config_file);
        validate_config(&config)?;

        Ok(config)
    }

    /// Render a redacted effective configuration as JSON.
    fn to_redacted_json(&self) -> Result<String, ConfigurationError> {
        Ok(serde_json::to_string_pretty(&self.redacted()?)?)
    }

    /// Render a redacted effective configuration as TOML.
    fn to_redacted_toml(&self) -> Result<String, ConfigurationError> {
        Ok(toml::to_string_pretty(&self.redacted()?)?)
    }

    /// Describe layered configuration precedence for operator reporting.
    pub const fn precedence_summary() -> &'static str {
        "CLI host/port > environment > config file > defaults"
    }

    fn redacted(&self) -> Result<RuntimeConfig, ConfigurationError> {
        // Clone into a printable snapshot so secrets can be redacted without mutating the live runtime config.
        let mut redacted = self.clone();
        redacted.database.url = Url::parse("postgres://redacted.invalid/redacted")?;
        Ok(redacted)
    }
}

/// CLI-selected listener overrides applied above environment and file layers.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BrokerRunOverrides {
    /// Optional listener address override.
    pub host: Option<IpAddr>,
    /// Optional listener port override.
    pub port: Option<u16>,
}

/// Effective configuration output selected by the CLI.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigurationOutputFormat {
    /// Pretty JSON output.
    Json,
    /// Pretty TOML output.
    Toml,
}

/// Result of configuration loading after the configuration owner has reported failures.
pub enum ConfigurationLoadOutcome {
    /// A validated configuration is ready for broker composition.
    Ready(Box<RuntimeConfig>),
    /// Loading failed and the contained process outcome is ready to return.
    Failed(ProcessExitOutcome),
}

/// Load configuration for broker execution and report a typed failure at this boundary.
pub fn load_reported(config_file: &Path, overrides: BrokerRunOverrides) -> ConfigurationLoadOutcome {
    match RuntimeConfig::load(
        config_file,
        ServerOverrides {
            host: overrides.host,
            port: overrides.port,
        },
    ) {
        Ok(config) => ConfigurationLoadOutcome::Ready(Box::new(config)),
        Err(error) => {
            eprintln!("{error}");
            ConfigurationLoadOutcome::Failed(ProcessExitOutcome::ConfigurationFailure)
        }
    }
}

/// Validate configuration and report the result at the configuration boundary.
pub fn validate_reported(config_file: &Path) -> ProcessExitOutcome {
    match RuntimeConfig::load(config_file, ServerOverrides::default()) {
        Ok(_) => {
            println!("Configuration valid: {}", config_file.display());
            ProcessExitOutcome::Success
        }
        Err(error) => {
            eprintln!("{error}");
            ProcessExitOutcome::ConfigurationFailure
        }
    }
}

/// Print the effective redacted configuration and report rendering failures at their owner.
pub fn print_effective_reported(config_file: &Path, format: ConfigurationOutputFormat) -> ProcessExitOutcome {
    let config = match RuntimeConfig::load(config_file, ServerOverrides::default()) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error}");
            return ProcessExitOutcome::ConfigurationFailure;
        }
    };
    let rendered = match format {
        ConfigurationOutputFormat::Json => config.to_redacted_json(),
        ConfigurationOutputFormat::Toml => config.to_redacted_toml(),
    };
    match rendered {
        Ok(rendered) => {
            println!("{rendered}");
            ProcessExitOutcome::Success
        }
        Err(error) => {
            eprintln!("{error}");
            ProcessExitOutcome::ConfigurationFailure
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::config::observability::{LogLevel, MetricsExposure};
    use andromeda_entity::{config::EntityConfig, spatial_config::SpatialConfig};
    use andromeda_federation::config::{FederationConfig, SplitEntitiesDefault};
    use andromeda_http_server::config::ServerConfig;
    use andromeda_http_source_identity::config::InfoConfig;
    use andromeda_http_stack::config::{CorsMode, HttpFeatureMode, HttpStackConfig};
    use andromeda_http_subscription::config::{NotificationConfig, SseConfig, WsConfig};
    use andromeda_ngsi_ld_lexicon::{context_source_pseudonym::ContextSourcePseudonym, uri::Uri};
    use andromeda_notification::delivery_config::DeliveryConfig;
    use andromeda_postgresql_atomic_write::config::{AtomicWriteConfig, BulkWriteMode};
    use andromeda_postgresql_context_store::{
        config::{DatabaseConfig, SynchronousCommit},
        maintenance_config::MaintenanceConfig,
    };
    use andromeda_snapshot::config::SnapshotConfig;
    use andromeda_temporal::config::TemporalAppendMode;
    use serial_test::serial;
    use std::{
        collections::HashMap,
        env,
        fs,
        net::IpAddr,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;
    use andromeda_tenancy::db_name_prefix::DbNamePrefix;

    fn test_runtime_config_with_preload(path: &str) -> RuntimeConfig {
        let mut preloads = HashMap::new();
        preloads.insert("https://example.com/context.jsonld".parse().expect("test URL"), path.into());
        RuntimeConfig {
            server: ServerConfig {
                host: "127.0.0.1".parse().unwrap(),
                port: 1056,
                graceful_drain_secs: 30,
            },
            http: HttpStackConfig {
                request_timeout_secs: 30,
                response_compression: HttpFeatureMode::Enabled,
                request_decompression: HttpFeatureMode::Enabled,
                cors: CorsMode::Permissive,
                max_request_body_bytes: 67_108_864,
            },
            database: DatabaseConfig {
                url: "postgres://postgres:postgres@localhost:5432/postgres".parse().expect("database URL"),
                max_connections: 32,
                synchronous_commit: SynchronousCommit::Off,
                preload_name_dictionaries: true,
                context_hit_flush_secs: 5,
                maintenance: MaintenanceConfig::default(),
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
                append_mode: TemporalAppendMode::Native,
                default_last_n: 19,
                expiration_check_interval_secs: 30,
                intent_fallback_secs: 1,
                intent_batch_size: 64,
                intent_lease_secs: 30,
                intent_retry_secs: 2,
                intent_retry_max_secs: 300,
                intent_stuck_after_attempts: 10,
                chunk_interval_days: None,
                columnstore_after_days: None,
                retention_horizon_days: None,
                retention_sweep_interval_secs: 3600,
            },
            tenancy: TenancyConfig {
                enabled: true,
                tenant_db_prefix: DbNamePrefix::try_from("andromeda_tenant").expect("valid default db name prefix"),
                tenant_max_connections: 16,
                runtime_idle_ttl_seconds: 900,
                runtime_max_cached: 64,
                janitor_interval_seconds: 30,
            },
            info: InfoConfig {
                source_id: "urn:ngsi-ld:ContextSourceIdentity:andromeda".parse::<Uri>().expect("valid source ID"),
                source_alias: "andromeda".parse::<ContextSourcePseudonym>().expect("valid source alias"),
                source_extras: None,
                advertised_url: None,
            },
            entity: EntityConfig {
                batch_concurrency: None,
                batch_chunk_concurrency: None,
                batch_chunk_write_concurrency: None,
                batch_chunk_size: 512,
                batch_large_threshold: 512,
                batch_prepare_blocking_threshold: 128,
                batch_async_side_effect_threshold: None,
                expiry_batch_size: 1_000,
                pending_side_effects_poll_secs: 1,
                pending_side_effects_lease_secs: 30,
                pending_side_effects_batch_size: 32,
                pending_side_effects_max_attempts: 5,
                pending_side_effects_base_backoff_secs: 2,
                pending_side_effects_dead_letter_after_secs: 604_800,
                pending_side_effects_cleanup_interval_secs: 300,
            },
            federation: FederationConfig {
                forward_concurrency: 16,
                connect_timeout_ms: 3000,
                request_timeout_ms: 5000,
                cache_max_entries: 1024,
                split_entities_default: SplitEntitiesDefault::Complete,
            },
            postgresql_write: AtomicWriteConfig {
                bulk_mode: BulkWriteMode::Unnest,
            },
            snapshot: SnapshotConfig { preparation_concurrency: 4 },
            notification_delivery: DeliveryConfig {
                enabled: false,
                max_attempts: 5,
                base_backoff_secs: 5,
                poll_interval_secs: 1,
                batch_size: 50,
                concurrency: 20,
                lease_secs: 30,
                error_max_chars: 1024,
                cleanup_after_secs: 86400,
                cleanup_interval_secs: 300,
                dead_letter_after_secs: 604_800,
                orphan_event_after_secs: 86400,
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
            spatial: SpatialConfig {
                rebuild_interval_secs: 1800,
                change_channel_capacity: 65_536,
                lag_log_interval_secs: 5,
            },
            observability: ObservabilityConfig {
                log_level: LogLevel::Info,
                metrics: MetricsExposure::Disabled,
            },
        }
    }

    fn write_temp_config(contents: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after unix epoch")
            .as_nanos();
        let path = env::temp_dir().join(format!("andromeda-config-{unique}.toml"));
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
            .get(&"https://example.com/context.jsonld".parse().expect("test URL"))
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
            .get(&"https://example.com/context.jsonld".parse().expect("test URL"))
            .expect("preload path missing");
        pretty_assertions::assert_eq!(path, "/opt/contexts/test.jsonld");
    }

    #[test]
    #[serial]
    fn env_overrides_default_host() {
        unsafe { env::set_var("ANDROMEDA_SERVER_HOST", "192.168.1.1") };
        let config = RuntimeConfig::builder().env().load().expect("config should load");
        unsafe { env::remove_var("ANDROMEDA_SERVER_HOST") };
        pretty_assertions::assert_eq!(config.server.host, IpAddr::from([192, 168, 1, 1]));
    }

    #[test]
    #[serial]
    fn env_overrides_default_port() {
        unsafe { env::set_var("ANDROMEDA_SERVER_PORT", "9999") };
        let config = RuntimeConfig::builder().env().load().expect("config should load");
        unsafe { env::remove_var("ANDROMEDA_SERVER_PORT") };
        pretty_assertions::assert_eq!(config.server.port, 9999);
    }

    #[test]
    #[serial]
    fn env_overrides_database_url() {
        unsafe { env::set_var("ANDROMEDA_DATABASE_URL", "postgres://envhost/envdb") };
        let config = RuntimeConfig::builder().env().load().expect("config should load");
        unsafe { env::remove_var("ANDROMEDA_DATABASE_URL") };
        pretty_assertions::assert_eq!(config.database.url.as_str(), "postgres://envhost/envdb");
    }

    #[test]
    #[serial]
    fn env_overrides_jsonld_cache_capacity() {
        unsafe { env::set_var("ANDROMEDA_JSONLD_CACHE_CAPACITY", "9999") };
        let config = RuntimeConfig::builder().env().load().expect("config should load");
        unsafe { env::remove_var("ANDROMEDA_JSONLD_CACHE_CAPACITY") };
        pretty_assertions::assert_eq!(config.jsonld.cache_capacity, 9999);
    }

    #[test]
    #[serial]
    fn env_parses_http_response_compression_mode() {
        unsafe { env::set_var("ANDROMEDA_HTTP_RESPONSE_COMPRESSION", "disabled") };
        let config = RuntimeConfig::builder().env().load().expect("config should load");
        unsafe { env::remove_var("ANDROMEDA_HTTP_RESPONSE_COMPRESSION") };
        pretty_assertions::assert_eq!(config.http.response_compression, HttpFeatureMode::Disabled);
    }

    #[test]
    #[serial]
    fn env_parses_http_cors_mode() {
        unsafe { env::set_var("ANDROMEDA_HTTP_CORS", "disabled") };
        let config = RuntimeConfig::builder().env().load().expect("config should load");
        unsafe { env::remove_var("ANDROMEDA_HTTP_CORS") };
        pretty_assertions::assert_eq!(config.http.cors, CorsMode::Disabled);
    }

    #[test]
    #[serial]
    fn env_parses_json_maps_for_hashmap_fields() {
        unsafe {
            env::set_var(
                "ANDROMEDA_JSONLD_PRELOAD_CONTEXTS",
                "{\"https://example.com/context.jsonld\":\"contexts/test.jsonld\"}",
            )
        };
        let config = RuntimeConfig::builder().env().load().expect("config should load");
        unsafe { env::remove_var("ANDROMEDA_JSONLD_PRELOAD_CONTEXTS") };
        pretty_assertions::assert_eq!(
            config
                .jsonld
                .preload_contexts
                .get(&"https://example.com/context.jsonld".parse().expect("test URL")),
            Some(&"contexts/test.jsonld".into())
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
    fn maintenance_modes_and_profiles_load_from_the_database_section() {
        for (mode, profile) in [
            ("disabled", "conservative"),
            ("disabled", "responsive"),
            ("observe", "conservative"),
            ("observe", "responsive"),
            ("automatic", "conservative"),
            ("automatic", "responsive"),
        ] {
            let path = write_temp_config(&format!("[database.maintenance]\nmode = \"{mode}\"\nprofile = \"{profile}\"\n"));
            let config = RuntimeConfig::load(&path, ServerOverrides::default()).expect("maintenance config should load");
            fs::remove_file(path).expect("temp config should be removed");
            assert_eq!(config.database.maintenance.mode.to_string(), mode);
            assert_eq!(config.database.maintenance.profile.to_string(), profile);
        }
    }

    #[test]
    #[serial]
    fn invalid_maintenance_values_are_rejected() {
        for body in [
            "[database.maintenance]\nmode = \"enabled\"\n",
            "[database.maintenance]\nprofile = \"aggressive\"\n",
        ] {
            let path = write_temp_config(body);
            let result = RuntimeConfig::load(&path, ServerOverrides::default());
            fs::remove_file(path).expect("temp config should be removed");
            assert!(result.is_err(), "{body}");
        }
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

        unsafe { env::set_var("ANDROMEDA_SERVER_PORT", "1700") };
        let config = RuntimeConfig::load(&path, ServerOverrides::default()).expect("config should load");
        unsafe { env::remove_var("ANDROMEDA_SERVER_PORT") };
        fs::remove_file(path).expect("temp config should be removed");

        pretty_assertions::assert_eq!(config.server.port, 1700);
    }

    #[test]
    #[serial]
    fn cli_overrides_env() {
        unsafe { env::set_var("ANDROMEDA_SERVER_PORT", "8888") };
        let config = RuntimeConfig::load(Path::new(DEFAULT_CONFIG_PATH), ServerOverrides { host: None, port: Some(7777) }).expect("config should load");
        unsafe { env::remove_var("ANDROMEDA_SERVER_PORT") };

        pretty_assertions::assert_eq!(config.server.port, 7777);
    }

    #[test]
    #[serial]
    fn legacy_env_names_are_ignored() {
        unsafe { env::set_var("ANDROMEDA_PORT", "8888") };
        let config = RuntimeConfig::builder().env().load().expect("config should load");
        unsafe { env::remove_var("ANDROMEDA_PORT") };

        pretty_assertions::assert_eq!(config.server.port, 1056);
    }

    #[test]
    fn redacted_effective_config_hides_secrets() {
        let mut config = test_runtime_config_with_preload("contexts/test.jsonld");
        config.database.url = "postgres://secret".parse().expect("database URL");

        let rendered = config.to_redacted_toml().expect("redacted toml should render");

        assert!(!rendered.contains("postgres://secret"));
        assert!(rendered.contains("redacted.invalid"));
        assert!(rendered.contains("[database.maintenance]"));
        assert!(rendered.contains("mode = \"disabled\""));
        assert!(rendered.contains("profile = \"conservative\""));
    }

    #[test]
    fn validate_rejects_zero_spatial_rebuild_interval() {
        let mut config = test_runtime_config_with_preload("contexts/test.jsonld");
        config.spatial.rebuild_interval_secs = 0;

        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn validate_rejects_zero_spatial_change_channel_capacity() {
        let mut config = test_runtime_config_with_preload("contexts/test.jsonld");
        config.spatial.change_channel_capacity = 0;

        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn validate_rejects_zero_spatial_lag_log_interval_secs() {
        let mut config = test_runtime_config_with_preload("contexts/test.jsonld");
        config.spatial.lag_log_interval_secs = 0;

        assert!(validate_config(&config).is_err());
    }

    #[test]
    #[serial]
    fn env_overrides_spatial_change_channel_capacity() {
        unsafe { env::set_var("ANDROMEDA_SPATIAL_CHANGE_CHANNEL_CAPACITY", "12345") };
        let config = RuntimeConfig::builder().env().load().expect("config should load");
        unsafe { env::remove_var("ANDROMEDA_SPATIAL_CHANGE_CHANNEL_CAPACITY") };
        pretty_assertions::assert_eq!(config.spatial.change_channel_capacity, 12345);
    }

    #[test]
    fn validate_rejects_zero_http_request_timeout() {
        let mut config = test_runtime_config_with_preload("contexts/test.jsonld");
        config.http.request_timeout_secs = 0;

        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn validate_rejects_zero_batch_chunk_write_concurrency() {
        let mut config = test_runtime_config_with_preload("contexts/test.jsonld");
        config.entity.batch_chunk_write_concurrency = Some(0);

        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn validate_rejects_zero_entity_expiry_batch_size() {
        let mut config = test_runtime_config_with_preload("contexts/test.jsonld");
        config.entity.expiry_batch_size = 0;

        assert!(validate_config(&config).is_err());
    }

    #[test]
    #[serial]
    fn env_overrides_batch_chunk_write_concurrency() {
        unsafe { env::set_var("ANDROMEDA_ENTITY_BATCH_CHUNK_WRITE_CONCURRENCY", "9") };
        let config = RuntimeConfig::builder().env().load().expect("config should load");
        unsafe { env::remove_var("ANDROMEDA_ENTITY_BATCH_CHUNK_WRITE_CONCURRENCY") };
        pretty_assertions::assert_eq!(config.entity.batch_chunk_write_concurrency, Some(9));
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
