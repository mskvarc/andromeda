use crate::{
    config::{
        error::ConfigurationError,
        notifications::NotificationsConfig,
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
use andromeda_jsonld::config::JsonLdConfig;
use andromeda_postgresql_atomic_write::config::AtomicWriteConfig;
use andromeda_postgresql_context_store::config::DatabaseConfig;
use andromeda_snapshot::config::SnapshotConfig;
use andromeda_temporal::config::TemporalConfig;
use andromeda_tenancy::config::TenancyConfig;
use figment2::{
    Figment,
    providers::{Env, Format, Serialized, Toml},
    value::Value,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    net::IpAddr,
    path::{Path, PathBuf},
};
use toml_comment::TomlComment;
use url::Url;

/// Default broker configuration path used by the CLI.
pub const DEFAULT_CONFIG_PATH: &str = "config/andromeda.toml";

/// Fully loaded broker construction configuration.
#[derive(Deserialize, Serialize, Default, TomlComment, Debug, Clone)]
pub struct RuntimeConfig {
    /// HTTP listener and graceful-drain configuration.
    pub server: ServerConfig,

    /// Process-wide inbound HTTP stack configuration.
    pub http: HttpStackConfig,

    /// Primary and tenant PostgreSQL connection configuration.
    pub database: DatabaseConfig,

    /// JSON-LD processing and context-loading configuration.
    pub jsonld: JsonLdConfig,

    /// Temporal policy and append-mode configuration.
    pub temporal: TemporalConfig,

    /// Tenant creation and runtime-cache configuration.
    pub tenancy: TenancyConfig,

    /// Context Source identity configuration.
    pub identity: InfoConfig,

    /// Entity batching, expiry, and durable side-effect configuration.
    pub entity: EntityConfig,

    /// Distributed Context Source forwarding configuration.
    pub federation: FederationConfig,

    /// Atomic-write strategy.
    pub atomic_write: AtomicWriteConfig,

    /// Snapshot operation configuration.
    pub snapshot: SnapshotConfig,

    /// Durable-delivery and streaming notification configuration.
    pub notifications: NotificationsConfig,

    /// Spatial projection and reconciliation configuration.
    pub spatial: SpatialConfig,

    /// Composition-owned tracing configuration.
    pub observability: ObservabilityConfig,
}

impl RuntimeConfig {
    /// Load, normalize, and validate the configured broker resources.
    ///
    /// The layers, in increasing precedence, are the built-in defaults, the
    /// TOML file (silently absent when the path does not exist), the
    /// `ANDROMEDA__`-prefixed environment, and finally the CLI listener
    /// overrides. Environment nesting uses `__` between every level, so
    /// `ANDROMEDA__NOTIFICATIONS__STREAMING__WS__PING_INTERVAL_SECS` reaches
    /// `notifications.streaming.ws.ping_interval_secs`; a single underscore separates the
    /// words of one key and never nests.
    fn load(config_file: &Path, overrides: ServerOverrides) -> Result<RuntimeConfig, ConfigurationError> {
        // A value is JSON-parsed first so that a map-valued or structured
        // setting (the JSON-LD preload map, the raw `contextSourceExtras`) can
        // be given as one environment value; anything that is not JSON stays
        // the literal string it was, which is how scalars and enum tokens
        // arrive.
        let environment = Env::prefixed("ANDROMEDA__")
            .split("__")
            .parser(|value| serde_json::from_str::<Value>(value).unwrap_or_else(|_| Value::from(value)));
        let mut figment = Figment::from(Serialized::defaults(RuntimeConfig::default()))
            .merge(Toml::file(config_file))
            .merge(environment);
        if let Some(host) = overrides.host {
            figment = figment.merge(Serialized::default("server.host", host));
        }
        if let Some(port) = overrides.port {
            figment = figment.merge(Serialized::default("server.port", port));
        }
        let mut config: RuntimeConfig = figment.extract().map_err(|error| ConfigurationError::Load(Box::new(error)))?;
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
    #[must_use]
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
#[must_use]
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
#[must_use]
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
#[must_use]
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

/// Emit a fully commented default configuration and report write failures at their owner.
///
/// With `output` set the commented TOML is written to that file; without it the
/// document is printed to standard output. Every emitted setting carries the
/// rustdoc of the field it comes from as a leading comment, and every field
/// left at its built-in default is shown with that default's value.
#[must_use]
pub fn generate_default_reported(output: Option<PathBuf>) -> ProcessExitOutcome {
    let rendered = <RuntimeConfig as TomlComment>::default_toml();
    let Some(path) = output else {
        print!("{rendered}");
        return ProcessExitOutcome::Success;
    };
    match fs::write(&path, rendered) {
        Ok(()) => {
            println!("Default configuration written to {}", path.display());
            ProcessExitOutcome::Success
        }
        Err(error) => {
            eprintln!("failed to write default configuration to {}: {error}", path.display());
            ProcessExitOutcome::ConfigurationFailure
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use andromeda_http_stack::config::{CorsMode, HttpFeatureMode};
    use serial_test::serial;
    use std::{
        env,
        fs,
        net::IpAddr,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    /// A configuration built from the defaults with one preload context, for the
    /// path-normalization, redaction and validation tests that vary one field.
    fn test_runtime_config_with_preload(path: &str) -> RuntimeConfig {
        let mut config = RuntimeConfig::default();
        config
            .jsonld
            .preload_contexts
            .insert("https://example.com/context.jsonld".parse().expect("test URL"), path.into());
        config
    }

    /// Load with only the built-in defaults and the environment, against a path
    /// that does not exist so no file layer contributes.
    fn load_env_only() -> RuntimeConfig {
        RuntimeConfig::load(Path::new("/nonexistent-andromeda.toml"), ServerOverrides::default()).expect("config should load")
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
        unsafe { env::set_var("ANDROMEDA__SERVER__HOST", "192.168.1.1") };
        let config = load_env_only();
        unsafe { env::remove_var("ANDROMEDA__SERVER__HOST") };
        pretty_assertions::assert_eq!(config.server.host, IpAddr::from([192, 168, 1, 1]));
    }

    #[test]
    #[serial]
    fn env_overrides_default_port() {
        unsafe { env::set_var("ANDROMEDA__SERVER__PORT", "9999") };
        let config = load_env_only();
        unsafe { env::remove_var("ANDROMEDA__SERVER__PORT") };
        pretty_assertions::assert_eq!(config.server.port, 9999);
    }

    #[test]
    #[serial]
    fn env_overrides_database_url() {
        unsafe { env::set_var("ANDROMEDA__DATABASE__URL", "postgres://envhost/envdb") };
        let config = load_env_only();
        unsafe { env::remove_var("ANDROMEDA__DATABASE__URL") };
        pretty_assertions::assert_eq!(config.database.url.as_str(), "postgres://envhost/envdb");
    }

    #[test]
    #[serial]
    fn env_overrides_jsonld_cache_capacity() {
        unsafe { env::set_var("ANDROMEDA__JSONLD__CACHE_CAPACITY", "9999") };
        let config = load_env_only();
        unsafe { env::remove_var("ANDROMEDA__JSONLD__CACHE_CAPACITY") };
        pretty_assertions::assert_eq!(config.jsonld.cache_capacity, 9999);
    }

    #[test]
    #[serial]
    fn env_parses_http_response_compression_mode() {
        unsafe { env::set_var("ANDROMEDA__HTTP__RESPONSE_COMPRESSION", "disabled") };
        let config = load_env_only();
        unsafe { env::remove_var("ANDROMEDA__HTTP__RESPONSE_COMPRESSION") };
        pretty_assertions::assert_eq!(config.http.response_compression, HttpFeatureMode::Disabled);
    }

    #[test]
    #[serial]
    fn env_parses_http_cors_mode() {
        unsafe { env::set_var("ANDROMEDA__HTTP__CORS", "disabled") };
        let config = load_env_only();
        unsafe { env::remove_var("ANDROMEDA__HTTP__CORS") };
        pretty_assertions::assert_eq!(config.http.cors, CorsMode::Disabled);
    }

    #[test]
    #[serial]
    fn env_parses_json_maps_for_hashmap_fields() {
        unsafe {
            env::set_var(
                "ANDROMEDA__JSONLD__PRELOAD_CONTEXTS",
                "{\"https://example.com/context.jsonld\":\"contexts/test.jsonld\"}",
            );
        };
        let config = load_env_only();
        unsafe { env::remove_var("ANDROMEDA__JSONLD__PRELOAD_CONTEXTS") };
        // The JSON object env value is parsed into the map; the stored path is then
        // normalized against the config file's directory, so it is asserted by suffix.
        let path = config
            .jsonld
            .preload_contexts
            .get(&"https://example.com/context.jsonld".parse().expect("test URL"));
        assert!(path.is_some_and(|path| path.ends_with("contexts/test.jsonld")), "{path:?}");
    }

    #[test]
    #[serial]
    fn file_overrides_defaults() {
        let path = write_temp_config(
            r"
[server]
port = 1600

[notifications.streaming.ws]
ping_interval_secs = 45
",
        );

        let config = RuntimeConfig::load(&path, ServerOverrides::default()).expect("config should load");
        fs::remove_file(path).expect("temp config should be removed");

        pretty_assertions::assert_eq!(config.server.port, 1600);
        pretty_assertions::assert_eq!(config.notifications.streaming.ws.ping_interval_secs, 45);
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
            r"
[server]
port = 1600
",
        );

        unsafe { env::set_var("ANDROMEDA__SERVER__PORT", "1700") };
        let config = RuntimeConfig::load(&path, ServerOverrides::default()).expect("config should load");
        unsafe { env::remove_var("ANDROMEDA__SERVER__PORT") };
        fs::remove_file(path).expect("temp config should be removed");

        pretty_assertions::assert_eq!(config.server.port, 1700);
    }

    #[test]
    #[serial]
    fn cli_overrides_env() {
        unsafe { env::set_var("ANDROMEDA__SERVER__PORT", "8888") };
        let config = RuntimeConfig::load(Path::new(DEFAULT_CONFIG_PATH), ServerOverrides { host: None, port: Some(7777) }).expect("config should load");
        unsafe { env::remove_var("ANDROMEDA__SERVER__PORT") };

        pretty_assertions::assert_eq!(config.server.port, 7777);
    }

    #[test]
    #[serial]
    fn legacy_env_names_are_ignored() {
        // Only the double-underscore contract nests; a single-prefix name and a
        // single underscore between section and key both fail to reach a field,
        // so the built-in default stands.
        unsafe { env::set_var("ANDROMEDA_PORT", "8888") };
        unsafe { env::set_var("ANDROMEDA__SERVER_HOST", "203.0.113.1") };
        let config = load_env_only();
        unsafe { env::remove_var("ANDROMEDA_PORT") };
        unsafe { env::remove_var("ANDROMEDA__SERVER_HOST") };

        pretty_assertions::assert_eq!(config.server.port, 1056);
        pretty_assertions::assert_eq!(config.server.host, IpAddr::from([0, 0, 0, 0]));
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
        unsafe { env::set_var("ANDROMEDA__SPATIAL__CHANGE_CHANNEL_CAPACITY", "12345") };
        let config = load_env_only();
        unsafe { env::remove_var("ANDROMEDA__SPATIAL__CHANGE_CHANNEL_CAPACITY") };
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
        unsafe { env::set_var("ANDROMEDA__ENTITY__BATCH_CHUNK_WRITE_CONCURRENCY", "9") };
        let config = load_env_only();
        unsafe { env::remove_var("ANDROMEDA__ENTITY__BATCH_CHUNK_WRITE_CONCURRENCY") };
        pretty_assertions::assert_eq!(config.entity.batch_chunk_write_concurrency, Some(9));
    }

    #[test]
    fn default_precedence_summary_matches_contract() {
        pretty_assertions::assert_eq!(RuntimeConfig::precedence_summary(), "CLI host/port > environment > config file > defaults");
    }

    #[test]
    fn default_toml_renders_sections_and_commented_fields() {
        let rendered = <RuntimeConfig as TomlComment>::default_toml();
        assert!(rendered.contains("[server]"), "{rendered}");
        assert!(rendered.contains("[database.maintenance]"), "{rendered}");
        assert!(rendered.lines().any(|line| line.starts_with('#')), "{rendered}");
    }
}
