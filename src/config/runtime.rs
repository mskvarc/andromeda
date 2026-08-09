use crate::{
    config::{
        context_source::InfoConfig,
        database::DatabaseConfig,
        http::HttpConfig,
        notification::NotificationConfig,
        observability::ObservabilityConfig,
        outbox::OutboxConfig,
        performance::PerformanceConfig,
        server::{ServerConfig, ServerOverrides},
        spatial::SpatialConfig,
        temporal::TemporalConfig,
        validation::{normalize_preload_context_paths, validate_config},
    },
    error::AndromedaError,
};
use andromeda_jsonld::config::JsonLdConfig;
use andromeda_tenancy::config::TenancyConfig;
use confique::{Config, Layer};
use serde::Serialize;
use std::path::Path;
use url::Url;

pub const DEFAULT_CONFIG_PATH: &str = "config/andromeda.toml";
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
        serde_json::to_string_pretty(&self.redacted()?)
            .map_err(|error| AndromedaError::internal(format!("failed to serialize effective config as JSON: {error}")))
    }

    pub fn to_redacted_toml(&self) -> Result<String, AndromedaError> {
        toml::to_string_pretty(&self.redacted()?).map_err(|error| AndromedaError::internal(format!("failed to serialize effective config as TOML: {error}")))
    }

    pub const fn precedence_summary() -> &'static str {
        "CLI host/port > environment > config file > defaults"
    }

    fn redacted(&self) -> Result<RuntimeConfig, AndromedaError> {
        // Clone into a printable snapshot so secrets can be redacted without mutating the live runtime config.
        let mut redacted = self.clone();
        redacted.database.url = Url::parse("postgres://redacted.invalid/redacted")
            .map_err(|error| AndromedaError::internal(format!("invalid built-in redacted database URL: {error}")))?;
        Ok(redacted)
    }
}

#[cfg(test)]
mod tests {
    use crate::config::{
        database::SynchronousCommit,
        http::{CorsMode, HttpFeatureMode},
        notification::{SseConfig, WsConfig},
        observability::LogLevel,
        performance::BulkWriteMode,
    };
    use andromeda_ngsi_ld_lexicon::{context_source_pseudonym::ContextSourcePseudonym, uri::Uri};
    use serial_test::serial;
    use std::{
        collections::HashMap,
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
            },
            http: HttpConfig {
                request_timeout_secs: 30,
                response_compression: HttpFeatureMode::Enabled,
                request_decompression: HttpFeatureMode::Enabled,
                cors: CorsMode::Permissive,
            },
            database: DatabaseConfig {
                url: "postgres://postgres:postgres@localhost:5432/postgres".parse().expect("database URL"),
                max_connections: 32,
                synchronous_commit: SynchronousCommit::Off,
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
                tenant_db_prefix: DbNamePrefix::try_from("andromeda_tenant").expect("valid default db name prefix"),
                tenant_max_connections: 16,
                runtime_idle_ttl_seconds: 900,
                runtime_max_cached: 64,
                janitor_interval_seconds: 30,
            },
            info: InfoConfig {
                source_id: "urn:ngsi-ld:ContextSourceIdentity:andromeda".parse::<Uri>().expect("valid source ID"),
                source_alias: "andromeda".parse::<ContextSourcePseudonym>().expect("valid source alias"),
            },
            performance: PerformanceConfig {
                batch_concurrency: None,
                batch_chunk_concurrency: None,
                batch_chunk_write_concurrency: None,
                preload_attribute_names: true,
                batch_chunk_size: 512,
                batch_large_threshold: 512,
                batch_prepare_blocking_threshold: 128,
                batch_side_effect_queue_capacity: 8,
                batch_async_side_effect_threshold: None,
                entity_expiry_batch_size: 1_000,
                pending_side_effects_poll_secs: 1,
                pending_side_effects_lease_secs: 30,
                pending_side_effects_batch_size: 32,
                pending_side_effects_max_attempts: 5,
                pending_side_effects_base_backoff_secs: 2,
                bulk_write_mode: BulkWriteMode::Unnest,
                max_request_body_bytes: 67_108_864,
                snapshot_concurrency: 4,
                forward_csr_concurrency: 16,
                context_hit_flush_secs: 5,
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
            spatial: SpatialConfig {
                rebuild_interval_secs: 1800,
                change_channel_capacity: 65_536,
                lag_log_interval_secs: 5,
            },
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
        pretty_assertions::assert_eq!(config.database.url.as_str(), "postgres://envhost/envdb");
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
        config.database.url = "postgres://secret".parse().expect("database URL");

        let rendered = config.to_redacted_toml().expect("redacted toml should render");

        assert!(!rendered.contains("postgres://secret"));
        assert!(rendered.contains("redacted.invalid"));
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
        unsafe { std::env::set_var("ANDROMEDA_SPATIAL_CHANGE_CHANNEL_CAPACITY", "12345") };
        let config = RuntimeConfig::builder().env().load().expect("config should load");
        unsafe { std::env::remove_var("ANDROMEDA_SPATIAL_CHANGE_CHANNEL_CAPACITY") };
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
        config.performance.batch_chunk_write_concurrency = Some(0);

        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn validate_rejects_zero_entity_expiry_batch_size() {
        let mut config = test_runtime_config_with_preload("contexts/test.jsonld");
        config.performance.entity_expiry_batch_size = 0;

        assert!(validate_config(&config).is_err());
    }

    #[test]
    #[serial]
    fn env_overrides_batch_chunk_write_concurrency() {
        unsafe { std::env::set_var("ANDROMEDA_PERFORMANCE_BATCH_CHUNK_WRITE_CONCURRENCY", "9") };
        let config = RuntimeConfig::builder().env().load().expect("config should load");
        unsafe { std::env::remove_var("ANDROMEDA_PERFORMANCE_BATCH_CHUNK_WRITE_CONCURRENCY") };
        pretty_assertions::assert_eq!(config.performance.batch_chunk_write_concurrency, Some(9));
    }

    #[test]
    fn default_precedence_summary_matches_contract() {
        pretty_assertions::assert_eq!(RuntimeConfig::precedence_summary(), "CLI host/port > environment > config file > defaults");
    }

    #[test]
    fn synchronous_commit_default_is_off() {
        let config = RuntimeConfig::builder().env().load().expect("config should load");
        pretty_assertions::assert_eq!(config.database.synchronous_commit, SynchronousCommit::Off);
    }

    #[test]
    #[serial]
    fn synchronous_commit_parses_all_modes() {
        let cases = [
            ("on", SynchronousCommit::On),
            ("off", SynchronousCommit::Off),
            ("local", SynchronousCommit::Local),
            ("remote-apply", SynchronousCommit::RemoteApply),
            ("remote-write", SynchronousCommit::RemoteWrite),
        ];
        for (raw, expected) in cases {
            unsafe { std::env::set_var("ANDROMEDA_DATABASE_SYNCHRONOUS_COMMIT", raw) };
            let config = RuntimeConfig::builder().env().load().expect("config should load");
            unsafe { std::env::remove_var("ANDROMEDA_DATABASE_SYNCHRONOUS_COMMIT") };
            pretty_assertions::assert_eq!(config.database.synchronous_commit, expected);
        }
    }

    #[test]
    #[serial]
    fn synchronous_commit_invalid_mode_errors() {
        unsafe { std::env::set_var("ANDROMEDA_DATABASE_SYNCHRONOUS_COMMIT", "enabled") };
        let result = RuntimeConfig::builder().env().load();
        unsafe { std::env::remove_var("ANDROMEDA_DATABASE_SYNCHRONOUS_COMMIT") };
        assert!(result.is_err(), "expected parse error for invalid synchronous_commit value");
    }

    #[test]
    fn batch_chunk_size_default_is_512() {
        let config = RuntimeConfig::builder().env().load().expect("config should load");
        pretty_assertions::assert_eq!(config.performance.batch_chunk_size, 512);
    }

    #[test]
    fn entity_expiry_batch_size_default_is_one_thousand() {
        let config = RuntimeConfig::builder().env().load().expect("config should load");
        pretty_assertions::assert_eq!(config.performance.entity_expiry_batch_size, 1_000);
    }

    #[test]
    fn synchronous_commit_renders_postgres_setting_strings() {
        pretty_assertions::assert_eq!(SynchronousCommit::On.as_postgres_setting(), "on");
        pretty_assertions::assert_eq!(SynchronousCommit::Off.as_postgres_setting(), "off");
        pretty_assertions::assert_eq!(SynchronousCommit::Local.as_postgres_setting(), "local");
        pretty_assertions::assert_eq!(SynchronousCommit::RemoteApply.as_postgres_setting(), "remote_apply");
        pretty_assertions::assert_eq!(SynchronousCommit::RemoteWrite.as_postgres_setting(), "remote_write");
    }

    #[test]
    fn config_layer_default_is_empty() {
        let layer = <RuntimeConfig as Config>::Layer::empty();
        assert!(layer.server.host.is_none());
        assert!(layer.server.port.is_none());
    }
}
