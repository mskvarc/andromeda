use crate::{
    config::{
        error::ConfigurationError,
        management::ManagementConfig,
        notifications::NotificationsConfig,
        observability::ObservabilityConfig,
        validation::{normalize_preload_context_paths, validate_config},
    },
    outcome::ProcessExitOutcome,
};
use andromeda_entity::{config::EntityConfig, spatial_config::SpatialConfig};
use andromeda_extensions::config::ExtensionsConfig;
use andromeda_federation::config::FederationConfig;
use andromeda_http_server::config::{ServerConfig, ServerOverrides};
use andromeda_http_source_identity::config::InfoConfig;
use andromeda_http_stack::config::HttpStackConfig;
use andromeda_http3_server::config::Http3Config;
use andromeda_jsonld::config::JsonLdConfig;
use andromeda_ngsi_ld_request::pagination::config::PaginationConfig;
use andromeda_postgresql_context_store::{
    config::DatabaseConfig,
    database_defaults::{DatabaseDefaults, StatementTimeout},
};
use andromeda_server_tls::config::TlsConfig;
use andromeda_snapshot::config::SnapshotConfig;
use andromeda_temporal::config::TemporalConfig;
use andromeda_tenancy::config::TenancyConfig;
use etcetera::{BaseStrategy, choose_base_strategy};
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

/// Application subdirectory holding Andromeda's files under a base directory.
const CONFIG_SUBDIR: &str = "andromeda";

/// Basename of the broker configuration file within the configuration directory.
const CONFIG_FILE_NAME: &str = "andromeda.toml";

/// Resolve the default broker configuration file under the platform
/// configuration directory.
///
/// The location follows platform convention through the XDG base-directory
/// strategy: `$XDG_CONFIG_HOME/andromeda/andromeda.toml` (falling back to
/// `~/.config/andromeda/andromeda.toml`) on Unix, `~/Library/Application
/// Support` on macOS, and `%APPDATA%` on Windows. No file is shipped, so the
/// resolved path is loaded only when it exists. `None` is returned when no home
/// directory can be determined, in which case the broker runs from defaults and
/// environment alone.
#[must_use]
pub fn default_config_file() -> Option<PathBuf> {
    let strategy = choose_base_strategy().ok()?;
    Some(strategy.config_dir().join(CONFIG_SUBDIR).join(CONFIG_FILE_NAME))
}

/// Decide which configuration file, if any, contributes a file layer.
///
/// An explicit `--config` path is honored verbatim and must exist. Without one,
/// the default file is used only when it is present; its absence is not an
/// error, leaving defaults and environment as the sole layers.
fn resolve_config_file(explicit: Option<PathBuf>) -> Result<Option<PathBuf>, ConfigurationError> {
    match explicit {
        Some(path) if path.exists() => Ok(Some(path)),
        Some(path) => Err(ConfigurationError::MissingFile(path)),
        None => Ok(default_config_file().filter(|path| path.exists())),
    }
}

/// Fully loaded broker construction configuration.
#[derive(Deserialize, Serialize, Default, TomlComment, Debug, Clone)]
pub struct RuntimeConfig {
    /// HTTP listener and graceful-drain configuration.
    pub server: ServerConfig,

    /// Process-wide inbound HTTP stack configuration.
    pub http: HttpStackConfig,

    /// Opt-in HTTP/3 (QUIC) listener configuration, sharing the TCP port.
    pub http3: Http3Config,

    /// The TLS identity every TLS-terminating listener presents; read only
    /// when some listener terminates TLS.
    pub tls: TlsConfig,

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

    /// Snapshot operation configuration.
    pub snapshot: SnapshotConfig,

    /// Durable-delivery and streaming notification configuration.
    pub notifications: NotificationsConfig,

    /// Spatial projection and reconciliation configuration.
    pub spatial: SpatialConfig,

    /// Composition-owned tracing configuration.
    pub observability: ObservabilityConfig,

    /// Management-plane topology and metrics-exposure configuration.
    pub management: ManagementConfig,

    /// Opt-in, non-standard extension configuration (off by default, but for
    /// cursor pagination).
    pub extensions: ExtensionsConfig,

    /// How a collection request that names neither `offset` nor `cursor` is
    /// paged.
    pub pagination: PaginationConfig,
}

impl RuntimeConfig {
    /// The session defaults installed on every database the broker serves
    /// from: the configured commit durability, and a statement timeout just
    /// past the HTTP request deadline.
    #[must_use]
    pub const fn database_defaults(&self) -> DatabaseDefaults {
        DatabaseDefaults {
            synchronous_commit: self.database.synchronous_commit,
            statement_timeout: StatementTimeout::past_request_deadline(self.http.request_timeout.duration()),
        }
    }

    /// Load, normalize, and validate the configured broker resources.
    ///
    /// The layers, in increasing precedence, are the built-in defaults, the
    /// TOML file (silently absent when the path does not exist), the
    /// `ANDROMEDA__`-prefixed environment, and finally the CLI listener
    /// overrides. Environment nesting uses `__` between every level, so
    /// `ANDROMEDA__NOTIFICATIONS__STREAMING__WS__PING_INTERVAL` reaches
    /// `notifications.streaming.ws.ping_interval`; a single underscore separates the
    /// words of one key and never nests.
    ///
    /// A `None` file means no TOML layer contributes: the broker loads from
    /// defaults and environment alone.
    fn load(config_file: Option<&Path>, overrides: ServerOverrides) -> Result<RuntimeConfig, ConfigurationError> {
        // A value is JSON-parsed first so that a map-valued or structured
        // setting (the JSON-LD preload map, the raw `contextSourceExtras`) can
        // be given as one environment value; anything that is not JSON stays
        // the literal string it was, which is how scalars and enum tokens
        // arrive.
        let environment = Env::prefixed("ANDROMEDA__")
            .split("__")
            .parser(|value| serde_json::from_str::<Value>(value).unwrap_or_else(|_| Value::from(value)));
        let mut figment = Figment::from(Serialized::defaults(RuntimeConfig::default()));
        if let Some(path) = config_file {
            figment = figment.merge(Toml::file(path));
        }
        figment = figment.merge(environment);
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
pub fn load_reported(config_file: Option<PathBuf>, overrides: BrokerRunOverrides) -> ConfigurationLoadOutcome {
    let config_file = match resolve_config_file(config_file) {
        Ok(config_file) => config_file,
        Err(error) => {
            eprintln!("{error}");
            return ConfigurationLoadOutcome::Failed(ProcessExitOutcome::ConfigurationFailure);
        }
    };
    match RuntimeConfig::load(
        config_file.as_deref(),
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
pub fn validate_reported(config_file: Option<PathBuf>) -> ProcessExitOutcome {
    let config_file = match resolve_config_file(config_file) {
        Ok(config_file) => config_file,
        Err(error) => {
            eprintln!("{error}");
            return ProcessExitOutcome::ConfigurationFailure;
        }
    };
    match RuntimeConfig::load(config_file.as_deref(), ServerOverrides::default()) {
        Ok(_) => {
            match config_file {
                Some(path) => println!("Configuration valid: {}", path.display()),
                None => println!("Configuration valid: defaults and environment (no configuration file present)"),
            }
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
pub fn print_effective_reported(config_file: Option<PathBuf>, format: ConfigurationOutputFormat) -> ProcessExitOutcome {
    let config_file = match resolve_config_file(config_file) {
        Ok(config_file) => config_file,
        Err(error) => {
            eprintln!("{error}");
            return ProcessExitOutcome::ConfigurationFailure;
        }
    };
    let config = match RuntimeConfig::load(config_file.as_deref(), ServerOverrides::default()) {
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
    use andromeda_configuration_scalars::human_duration::HumanDuration;
    use andromeda_entity::spatial_config::SpatialConsistency;
    use andromeda_http_stack::config::{compression::CompressionQuality, cors::CorsMode};
    use andromeda_ngsi_ld_request::pagination::paging_mode::PagingMode;
    use andromeda_postgresql_pool::write_reserve::WriteReserve;
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
        RuntimeConfig::load(None, ServerOverrides::default()).expect("config should load")
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
        normalize_preload_context_paths(&mut config, Some(Path::new("/tmp/andromeda/config/andromeda.toml")));
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
        normalize_preload_context_paths(&mut config, Some(Path::new("/tmp/andromeda/config/andromeda.toml")));
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
    fn env_parses_http_compression_quality() {
        unsafe { env::set_var("ANDROMEDA__HTTP__COMPRESSION__QUALITY", "best") };
        let config = load_env_only();
        unsafe { env::remove_var("ANDROMEDA__HTTP__COMPRESSION__QUALITY") };
        pretty_assertions::assert_eq!(config.http.compression.quality, CompressionQuality::Best);
    }

    #[test]
    #[serial]
    fn env_parses_http_cors_mode() {
        unsafe { env::set_var("ANDROMEDA__HTTP__CORS__MODE", "disabled") };
        let config = load_env_only();
        unsafe { env::remove_var("ANDROMEDA__HTTP__CORS__MODE") };
        pretty_assertions::assert_eq!(config.http.cors.mode, CorsMode::Disabled);
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
            r#"
[server]
port = 1600

[notifications.streaming.ws]
ping_interval = "45s"
"#,
        );

        let config = RuntimeConfig::load(Some(&path), ServerOverrides::default()).expect("config should load");
        fs::remove_file(path).expect("temp config should be removed");

        pretty_assertions::assert_eq!(config.server.port, 1600);
        pretty_assertions::assert_eq!(config.notifications.streaming.ws.ping_interval.duration().as_secs(), 45);
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
            let config = RuntimeConfig::load(Some(&path), ServerOverrides::default()).expect("maintenance config should load");
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
            let result = RuntimeConfig::load(Some(&path), ServerOverrides::default());
            fs::remove_file(path).expect("temp config should be removed");
            assert!(result.is_err(), "{body}");
        }
    }

    /// A claim lease that runs out while its notification is still in flight
    /// lets a second worker send the same notification, so a broker configured
    /// that way refuses to load rather than start.
    #[test]
    #[serial]
    fn a_delivery_lease_that_does_not_exceed_the_request_timeout_is_refused_at_load() {
        let path = write_temp_config(
            r#"
[notifications.delivery]
request_timeout = "30s"
lease = "30s"
"#,
        );

        let result = RuntimeConfig::load(Some(&path), ServerOverrides::default());
        fs::remove_file(path).expect("temp config should be removed");

        let error = result.expect_err("a lease equal to the request timeout must be refused");
        assert!(
            error
                .to_string()
                .contains("notifications.delivery.lease must exceed notifications.delivery.request_timeout"),
            "{error}"
        );
    }

    /// The delivery lease must leave its margin over one attempt's request
    /// timeout: one second short of it refuses to load, the margin exactly
    /// loads.
    #[test]
    #[serial]
    fn a_delivery_lease_is_refused_below_its_margin_over_the_request_timeout_and_loads_at_it() {
        let short = write_temp_config("[notifications.delivery]\nrequest_timeout = \"10s\"\nlease = \"14s\"\n");
        let refused = RuntimeConfig::load(Some(&short), ServerOverrides::default());
        fs::remove_file(short).expect("temp config should be removed");
        let exact = write_temp_config("[notifications.delivery]\nrequest_timeout = \"10s\"\nlease = \"15s\"\n");
        let loaded = RuntimeConfig::load(Some(&exact), ServerOverrides::default());
        fs::remove_file(exact).expect("temp config should be removed");

        let error = refused.expect_err("a 14s lease over a 10s request timeout lacks the margin");
        assert!(
            error
                .to_string()
                .contains("notifications.delivery.lease must exceed notifications.delivery.request_timeout by at least 5s"),
            "{error}"
        );
        loaded.expect("a 15s lease over a 10s request timeout carries the margin");
    }

    /// The side-effect lease must leave its margin over the connection acquire
    /// deadline: one second short of it refuses to load, the margin exactly
    /// loads.
    #[test]
    #[serial]
    fn a_side_effect_lease_is_refused_below_its_margin_over_the_acquire_timeout_and_loads_at_it() {
        let short = write_temp_config("[database]\nacquire_timeout = \"5s\"\n[entity]\npending_side_effects_lease = \"9s\"\n");
        let refused = RuntimeConfig::load(Some(&short), ServerOverrides::default());
        fs::remove_file(short).expect("temp config should be removed");
        let exact = write_temp_config("[database]\nacquire_timeout = \"5s\"\n[entity]\npending_side_effects_lease = \"10s\"\n");
        let loaded = RuntimeConfig::load(Some(&exact), ServerOverrides::default());
        fs::remove_file(exact).expect("temp config should be removed");

        let error = refused.expect_err("a 9s side-effect lease over a 5s acquire timeout lacks the margin");
        assert!(
            error
                .to_string()
                .contains("entity.pending_side_effects_lease (9s) must exceed database.acquire_timeout (5s) by at least 5s"),
            "{error}"
        );
        loaded.expect("a 10s side-effect lease over a 5s acquire timeout carries the margin");
    }

    /// The write reserve loads as a percentage below 100, defaults to a
    /// quarter, and a reserve of the whole pool refuses to load.
    #[test]
    #[serial]
    fn a_write_reserve_loads_below_the_whole_pool_and_the_whole_pool_is_refused() {
        let set = write_temp_config("[database]\nwrite_reserve_percent = 40\n");
        let loaded = RuntimeConfig::load(Some(&set), ServerOverrides::default());
        fs::remove_file(set).expect("temp config should be removed");
        let whole = write_temp_config("[database]\nwrite_reserve_percent = 100\n");
        let refused = RuntimeConfig::load(Some(&whole), ServerOverrides::default());
        fs::remove_file(whole).expect("temp config should be removed");

        let loaded = loaded.expect("a 40 percent reserve loads");
        assert_eq!(loaded.database.write_reserve_percent.read_connections(10), 6);
        assert_eq!(RuntimeConfig::default().database.write_reserve_percent, WriteReserve::DEFAULT);
        let error = refused.expect_err("a reserve of the whole pool leaves reads nothing");
        assert!(error.to_string().contains("write_reserve_percent"), "{error}");
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
        let config = RuntimeConfig::load(Some(&path), ServerOverrides::default()).expect("config should load");
        unsafe { env::remove_var("ANDROMEDA__SERVER__PORT") };
        fs::remove_file(path).expect("temp config should be removed");

        pretty_assertions::assert_eq!(config.server.port, 1700);
    }

    #[test]
    #[serial]
    fn cli_overrides_env() {
        unsafe { env::set_var("ANDROMEDA__SERVER__PORT", "8888") };
        let config = RuntimeConfig::load(None, ServerOverrides { host: None, port: Some(7777) }).expect("config should load");
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
        config.spatial.rebuild_interval = HumanDuration::from_secs(0);

        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn validate_rejects_zero_live_change_capacity() {
        let mut config = test_runtime_config_with_preload("contexts/test.jsonld");
        config.notifications.live_change_capacity = 0;

        assert!(validate_config(&config).is_err());
    }

    #[test]
    #[serial]
    fn env_overrides_notifications_live_change_capacity() {
        unsafe { env::set_var("ANDROMEDA__NOTIFICATIONS__LIVE_CHANGE_CAPACITY", "12345") };
        let config = load_env_only();
        unsafe { env::remove_var("ANDROMEDA__NOTIFICATIONS__LIVE_CHANGE_CAPACITY") };
        pretty_assertions::assert_eq!(config.notifications.live_change_capacity, 12345);
    }

    #[test]
    #[serial]
    fn env_overrides_notifications_live_evaluation_lanes() {
        unsafe { env::set_var("ANDROMEDA__NOTIFICATIONS__LIVE_EVALUATION_LANES", "7") };
        let config = load_env_only();
        unsafe { env::remove_var("ANDROMEDA__NOTIFICATIONS__LIVE_EVALUATION_LANES") };
        pretty_assertions::assert_eq!(config.notifications.live_evaluation_lanes.get(), 7);
    }

    #[test]
    #[serial]
    fn env_selects_the_spatial_consistency_mode_in_kebab_case() {
        unsafe { env::set_var("ANDROMEDA__SPATIAL__CONSISTENCY", "watermark") };
        let config = load_env_only();
        unsafe { env::remove_var("ANDROMEDA__SPATIAL__CONSISTENCY") };
        pretty_assertions::assert_eq!(config.spatial.consistency, SpatialConsistency::Watermark);
    }

    #[test]
    fn validate_rejects_zero_http_request_timeout() {
        let mut config = test_runtime_config_with_preload("contexts/test.jsonld");
        config.http.request_timeout = HumanDuration::from_secs(0);

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
    fn an_explicit_missing_config_file_is_rejected() {
        let path = PathBuf::from("/nonexistent-andromeda-explicit-check.toml");
        let resolved = resolve_config_file(Some(path.clone()));
        assert!(
            matches!(&resolved, Err(ConfigurationError::MissingFile(missing)) if *missing == path),
            "{resolved:?}"
        );
    }

    #[test]
    fn an_absent_default_config_file_is_not_an_error() {
        // Without an explicit path the default file contributes only when it
        // exists; its absence leaves defaults and environment as the only layers.
        assert!(resolve_config_file(None).is_ok());
    }

    #[test]
    fn the_default_config_file_lives_under_the_application_subdir() {
        // Skipped where no home directory is available (some CI containers), in
        // which case the broker runs from defaults and environment alone.
        if let Some(path) = default_config_file() {
            assert!(path.ends_with("andromeda/andromeda.toml"), "{}", path.display());
        }
    }

    #[test]
    fn default_toml_renders_sections_and_commented_fields() {
        let rendered = <RuntimeConfig as TomlComment>::default_toml();
        assert!(rendered.contains("[server]"), "{rendered}");
        assert!(rendered.contains("[database.maintenance]"), "{rendered}");
        assert!(rendered.lines().any(|line| line.starts_with('#')), "{rendered}");
    }

    /// History is written by one statement shape, so the configuration has no
    /// bulk-write strategy to choose.
    #[test]
    fn the_default_configuration_names_no_atomic_write_section() {
        let rendered = <RuntimeConfig as TomlComment>::default_toml();
        assert!(!rendered.contains("atomic_write") && !rendered.contains("bulk_mode"), "{rendered}");
    }

    #[test]
    fn default_toml_contains_the_management_table() {
        let rendered = <RuntimeConfig as TomlComment>::default_toml();
        assert!(rendered.contains("[management]"), "{rendered}");
        assert!(rendered.contains("mode = \"separate-port\""), "{rendered}");
        assert!(rendered.contains("port = 1086"), "{rendered}");
        assert!(rendered.contains("metrics = \"disabled\""), "{rendered}");
    }

    #[test]
    fn generated_config_renders_the_pagination_section() {
        let rendered = <RuntimeConfig as TomlComment>::default_toml();
        assert!(rendered.contains("[pagination]"), "{rendered}");
        assert!(rendered.contains("default_mode = \"cursor\""), "{rendered}");
        assert!(rendered.contains("cursor_pagination = \"enabled\""), "{rendered}");
    }

    #[test]
    #[serial]
    fn env_overrides_select_offset_default() {
        unsafe { env::set_var("ANDROMEDA__PAGINATION__DEFAULT_MODE", "offset") };
        let config = load_env_only();
        unsafe { env::remove_var("ANDROMEDA__PAGINATION__DEFAULT_MODE") };
        pretty_assertions::assert_eq!(config.pagination.default_mode, PagingMode::Offset);
    }

    #[test]
    #[serial]
    fn env_disabling_cursor_pagination_with_the_cursor_default_is_refused_at_load() {
        unsafe { env::set_var("ANDROMEDA__EXTENSIONS__CURSOR_PAGINATION", "disabled") };
        let loaded = RuntimeConfig::load(None, ServerOverrides::default());
        unsafe { env::remove_var("ANDROMEDA__EXTENSIONS__CURSOR_PAGINATION") };
        assert!(matches!(loaded, Err(ConfigurationError::Pagination(_))));
    }
}
