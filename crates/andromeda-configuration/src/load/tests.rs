//! Loading the layered configuration: defaults, file, environment and flags.

mod key_tree;

use crate::{
    broker_config::BrokerConfig,
    human_byte_size::HumanByteSize,
    load::{ListenerOverrides, LoadError, LoadedConfiguration, PRECEDENCE, default_configuration_file, load, resolve_configuration_file},
    redact::{redacted_json, redacted_toml},
    reference::template::configuration_template,
    schema::{
        content_coding::ContentCoding,
        database::{pooler::PoolerConfig, write_reserve::WriteReserve},
        entity::spatial_index::SpatialConsistency,
        http::{
            compression::CompressionQuality,
            cors::{CorsMode, MethodRule},
        },
        private_addresses::PrivateAddresses,
    },
    validate::{
        bound::{Bound, Quantity, Relation},
        config_key::{ConfigKey, QueueKey, QueueName},
        violation::ConfigurationViolation,
    },
};
use serial_test::serial;
use std::{
    env,
    fs,
    net::IpAddr,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// Load with only the built-in defaults and the environment, so no file layer
/// contributes.
fn load_env_only() -> BrokerConfig {
    load(None, ListenerOverrides::default()).expect("config should load").config
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

/// Load `contents` as the configuration file, with what the load ignores.
fn load_file_reporting(contents: &str) -> Result<LoadedConfiguration, LoadError> {
    let path = write_temp_config(contents);
    let result = load(Some(&path), ListenerOverrides::default());
    fs::remove_file(path).expect("temp config should be removed");
    result
}

/// Load `contents` as the configuration file.
fn load_file(contents: &str) -> Result<BrokerConfig, LoadError> {
    load_file_reporting(contents).map(|loaded| loaded.config)
}

/// Every setting `loaded` ignores, as its path and the layer that writes it.
fn ignored_settings_of(loaded: &LoadedConfiguration) -> Vec<(String, &'static str)> {
    loaded
        .ignored
        .iter()
        .map(|setting| (setting.path().to_string(), <&'static str>::from(setting)))
        .collect()
}

/// Every setting `contents`, loaded as the configuration file, writes that
/// the load ignores; the file must load.
fn ignored_in(contents: &str) -> Vec<(String, &'static str)> {
    let loaded = load_file_reporting(contents).expect("a file with unknown keys loads");
    ignored_settings_of(&loaded)
}

/// One ignored setting `path` that the configuration file writes.
fn in_file(path: &str) -> Vec<(String, &'static str)> {
    vec![(path.to_owned(), "file")]
}

/// The rules `contents` breaks as the configuration file: empty when it
/// loads, `None` when it is refused before the rules run.
fn violations_of(contents: &str) -> Option<Vec<ConfigurationViolation>> {
    match load_file(contents) {
        Ok(_) => Some(Vec::new()),
        Err(LoadError::Invalid(invalid)) => Some(invalid.into_violations()),
        Err(LoadError::MissingFile(_) | LoadError::Layers(_)) => None,
    }
}

#[test]
#[serial]
fn env_overrides_default_host() {
    temp_env::with_vars([("ANDROMEDA__SERVER__HOST", Some("192.168.1.1"))], || {
        let config = load_env_only();
        pretty_assertions::assert_eq!(config.server.host, IpAddr::from([192, 168, 1, 1]));
    });
}

#[test]
#[serial]
fn env_overrides_database_url() {
    temp_env::with_vars([("ANDROMEDA__DATABASE__URL", Some("postgres://envhost/envdb"))], || {
        let config = load_env_only();
        pretty_assertions::assert_eq!(config.database.url.as_str(), "postgres://envhost/envdb");
    });
}

#[test]
#[serial]
fn env_reaches_a_key_three_tables_deep() {
    temp_env::with_vars([("ANDROMEDA__ENTITY__SIDE_EFFECTS__STUCK_AFTER_ATTEMPTS", Some("64"))], || {
        let config = load_env_only();
        pretty_assertions::assert_eq!(config.entity.side_effects.stuck_after_attempts.get(), 64);
    });
}

#[test]
#[serial]
fn env_parses_enumerated_values_in_kebab_case() {
    temp_env::with_vars(
        [
            ("ANDROMEDA__HTTP__COMPRESSION__QUALITY", Some("best")),
            ("ANDROMEDA__HTTP__CORS__MODE", Some("disabled")),
            ("ANDROMEDA__ENTITY__SPATIAL_INDEX__CONSISTENCY", Some("store-verified")),
        ],
        || {
            let config = load_env_only();
            pretty_assertions::assert_eq!(config.http.compression.quality, CompressionQuality::Best);
            pretty_assertions::assert_eq!(config.http.cors.mode, CorsMode::Disabled);
            pretty_assertions::assert_eq!(config.entity.spatial_index.consistency, SpatialConsistency::StoreVerified);
        },
    );
}

/// A table is set in the file alone: an environment value is a scalar, so
/// JSON naming a table is text, which the table setting refuses.
#[test]
#[serial]
fn a_table_cannot_be_set_from_the_environment() {
    temp_env::with_vars(
        [(
            "ANDROMEDA__JSONLD__PRELOADED_CONTEXTS",
            Some("[{\"url\":\"https://example.com/context.jsonld\",\"path\":\"contexts/test.jsonld\"}]"),
        )],
        || {
            let result = load(None, ListenerOverrides::default());
            assert!(matches!(result, Err(LoadError::Layers(_))), "{result:?}");
        },
    );
}

#[test]
#[serial]
fn a_list_set_from_the_environment_splits_on_commas() {
    temp_env::with_vars([("ANDROMEDA__HTTP__COMPRESSION__ALGORITHMS", Some("gzip,zstd"))], || {
        let config = load_env_only();
        pretty_assertions::assert_eq!(config.http.compression.algorithms, ContentCoding::Gzip | ContentCoding::Zstd);
    });
}

#[test]
#[serial]
fn a_cors_method_list_set_from_the_environment_splits_on_commas() {
    temp_env::with_vars(
        [
            ("ANDROMEDA__HTTP__CORS__MODE", Some("configured")),
            ("ANDROMEDA__HTTP__CORS__ALLOW_METHODS", Some("GET,POST")),
        ],
        || {
            let config = load_env_only();
            assert!(
                matches!(&config.http.cors.allow_methods, MethodRule::List(methods) if methods.len() == 2),
                "{:?}",
                config.http.cors.allow_methods
            );
        },
    );
}

/// The defaults written by `configuration generate` load back to the
/// defaults, and every key the template writes is one the broker reads.
#[test]
#[serial]
fn the_generated_defaults_load_back_to_the_defaults() {
    let loaded = load_file_reporting(&configuration_template().expect("the template renders")).expect("the generated defaults load");
    pretty_assertions::assert_eq!(ignored_settings_of(&loaded), Vec::new());
    pretty_assertions::assert_eq!(
        toml::to_string(&loaded.config).expect("the loaded configuration renders"),
        toml::to_string(&BrokerConfig::default()).expect("the defaults render")
    );
}

#[test]
fn redaction_hides_the_direct_and_the_pooler_database_url() {
    let mut config = BrokerConfig::default();
    config.database.url = "postgres://user:direct-secret@db/andromeda".parse().expect("database URL");
    config.database.pooler = Some(PoolerConfig {
        url: "postgres://user:pooler-secret@pooler/andromeda".parse().expect("pooler URL"),
        ..PoolerConfig::default()
    });
    let rendered = redacted_toml(&config).expect("redacted toml should render");
    assert!(!rendered.contains("direct-secret") && !rendered.contains("pooler-secret"), "{rendered}");
    assert!(rendered.contains("redacted.invalid"), "{rendered}");
    let json = redacted_json(&config).expect("redacted json should render");
    assert!(!json.contains("direct-secret") && !json.contains("pooler-secret"), "{json}");
}

#[test]
#[serial]
fn file_overrides_defaults() {
    let config = load_file("[server]\nport = 1600\n\n[notifications.periodic]\nmax_poll_interval = \"45s\"\n").expect("config should load");
    pretty_assertions::assert_eq!(config.server.port, 1600);
    pretty_assertions::assert_eq!(config.notifications.periodic.max_poll_interval.duration().as_secs(), 45);
}

#[test]
#[serial]
fn maintenance_modes_and_profiles_load_from_the_database_table() {
    for (mode, profile) in [("disabled", "conservative"), ("observe", "responsive"), ("automatic", "conservative")] {
        let config = load_file(&format!("[database.maintenance]\nmode = \"{mode}\"\nprofile = \"{profile}\"\n")).expect("maintenance config should load");
        assert_eq!(config.database.maintenance.mode.to_string(), mode);
        assert_eq!(config.database.maintenance.profile.to_string(), profile);
    }
    assert!(load_file("[database.maintenance]\nprofile = \"aggressive\"\n").is_err());
}

#[test]
#[serial]
fn repack_modes_and_an_unlimited_relation_size_load_from_the_maintenance_table() {
    for mode in ["disabled", "observe", "automatic"] {
        let config = load_file(&format!(
            "[database.maintenance.repack]\nmode = \"{mode}\"\nmax_relation_size = \"unlimited\"\n"
        ))
        .expect("repack config should load");
        pretty_assertions::assert_eq!(config.database.maintenance.repack.mode.to_string(), mode);
        pretty_assertions::assert_eq!(config.database.maintenance.repack.max_relation_size.limit(), None);
    }
    let sized = load_file("[database.maintenance.repack]\nmax_relation_size = \"16 GiB\"\n").expect("repack config should load");
    pretty_assertions::assert_eq!(
        sized.database.maintenance.repack.max_relation_size.limit().map(HumanByteSize::as_u64),
        Some(16 << 30)
    );
    pretty_assertions::assert_eq!(sized.database.maintenance.repack.mode.to_string(), "observe");
}

#[test]
#[serial]
fn a_repack_relation_size_of_no_bytes_is_refused_as_the_file_loads() {
    pretty_assertions::assert_eq!(
        violations_of("[database.maintenance.repack]\nmax_relation_size = 0\n"),
        Some(vec![ConfigurationViolation::Zero {
            key: ConfigKey::DatabaseMaintenanceRepackMaxRelationSize
        }])
    );
}

/// The delivery lease must leave its margin over one attempt's request
/// timeout: a millisecond short of it refuses to load, the margin exactly
/// loads.
#[test]
#[serial]
fn a_delivery_lease_is_refused_below_its_margin_over_the_request_timeout_and_loads_at_it() {
    let refused = violations_of("[notifications.delivery]\nrequest_timeout = \"10s\"\n[notifications.delivery.queue]\nlease_timeout = \"14999ms\"\n");
    assert_eq!(
        refused,
        Some(vec![ConfigurationViolation::OutOfRange {
            key: ConfigKey::Queue(QueueName::NotificationDelivery, QueueKey::LeaseTimeout),
            value: Quantity::Duration(Duration::from_millis(14_999)),
            relation: Relation::AtLeast,
            bound: Bound::SettingPlusMargin {
                key: ConfigKey::NotificationsDeliveryRequestTimeout,
                value: Duration::from_secs(10),
                margin: Duration::from_secs(5),
            },
        }])
    );
    let loaded = violations_of("[notifications.delivery]\nrequest_timeout = \"10s\"\n[notifications.delivery.queue]\nlease_timeout = \"15s\"\n");
    assert_eq!(loaded, Some(Vec::new()));
}

/// A stopping tenant must be allowed at least one delivery deadline: a
/// millisecond short of the request timeout refuses to load, the timeout
/// exactly loads.
#[test]
#[serial]
fn a_shutdown_drain_is_refused_below_the_request_timeout_and_loads_at_it() {
    let refused = violations_of("[notifications.delivery]\nrequest_timeout = \"10s\"\n[notifications.evaluation]\nshutdown_drain = \"9999ms\"\n");
    assert_eq!(
        refused,
        Some(vec![ConfigurationViolation::OutOfRange {
            key: ConfigKey::NotificationsEvaluationShutdownDrain,
            value: Quantity::Duration(Duration::from_millis(9_999)),
            relation: Relation::AtLeast,
            bound: Bound::Setting {
                key: ConfigKey::NotificationsDeliveryRequestTimeout,
                value: Quantity::Duration(Duration::from_secs(10)),
            },
        }])
    );
    let loaded = violations_of("[notifications.delivery]\nrequest_timeout = \"10s\"\n[notifications.evaluation]\nshutdown_drain = \"10s\"\n");
    assert_eq!(loaded, Some(Vec::new()));
}

/// The side-effect lease must leave its margin over the connection acquire
/// deadline: a millisecond short of it refuses to load, the margin exactly
/// loads.
#[test]
#[serial]
fn a_side_effect_lease_is_refused_below_its_margin_over_the_acquire_timeout_and_loads_at_it() {
    let refused = violations_of("[database]\nacquire_timeout = \"5s\"\n[entity.side_effects.queue]\nlease_timeout = \"9999ms\"\n");
    assert_eq!(
        refused,
        Some(vec![ConfigurationViolation::OutOfRange {
            key: ConfigKey::Queue(QueueName::EntitySideEffects, QueueKey::LeaseTimeout),
            value: Quantity::Duration(Duration::from_millis(9_999)),
            relation: Relation::AtLeast,
            bound: Bound::SettingPlusMargin {
                key: ConfigKey::DatabaseAcquireTimeout,
                value: Duration::from_secs(5),
                margin: Duration::from_secs(5),
            },
        }])
    );
    let loaded = violations_of("[database]\nacquire_timeout = \"5s\"\n[entity.side_effects.queue]\nlease_timeout = \"10s\"\n");
    assert_eq!(loaded, Some(Vec::new()));
}

/// A file breaking three rules in three tables is refused for all three, and
/// the message lists each on its own line.
#[test]
#[serial]
fn a_file_breaking_three_rules_is_refused_for_all_three() {
    let contents = "[server]\nport = 0\n[expiry]\nsweep_interval = \"0s\"\n[temporal.query]\nmax_last_n = 5001\n";
    assert_eq!(
        violations_of(contents),
        Some(vec![
            ConfigurationViolation::Zero { key: ConfigKey::ServerPort },
            ConfigurationViolation::Zero {
                key: ConfigKey::ExpirySweepInterval
            },
            ConfigurationViolation::OutOfRange {
                key: ConfigKey::TemporalQueryMaxLastN,
                value: Quantity::Count(5_001),
                relation: Relation::AtMost,
                bound: Bound::Fixed(Quantity::Count(5_000)),
            },
        ])
    );
    let message = load_file(contents).map(|_| ()).map_err(|error| error.to_string());
    assert_eq!(
        message,
        Err("invalid configuration:\n  server.port must be greater than zero\n  expiry.sweep_interval must be greater than zero\n  temporal.query.max_last_n (5001) must be at most 5000".to_owned())
    );
}

/// The write reserve loads as a percentage below 100, defaults to a
/// quarter, and a reserve of the whole pool refuses to load.
#[test]
#[serial]
fn a_write_reserve_loads_below_the_whole_pool_and_the_whole_pool_is_refused() {
    let loaded = load_file("[database]\nwrite_reserve_percent = 40\n").expect("a 40 percent reserve loads");
    assert_eq!(loaded.database.write_reserve_percent.read_connections(10), 6);
    assert_eq!(BrokerConfig::default().database.write_reserve_percent, WriteReserve::DEFAULT);
    let error = load_file("[database]\nwrite_reserve_percent = 100\n").expect_err("a reserve of the whole pool leaves reads nothing");
    assert!(error.to_string().contains("write_reserve_percent"), "{error}");
}

#[test]
#[serial]
fn env_overrides_file() {
    let path = write_temp_config("[server]\nport = 1600\n");
    temp_env::with_vars([("ANDROMEDA__SERVER__PORT", Some("1700"))], || {
        let config = load(Some(&path), ListenerOverrides::default()).expect("config should load").config;
        fs::remove_file(path).expect("temp config should be removed");
        pretty_assertions::assert_eq!(config.server.port, 1700);
    });
}

#[test]
#[serial]
fn cli_overrides_env() {
    temp_env::with_vars([("ANDROMEDA__SERVER__PORT", Some("8888"))], || {
        let config = load(None, ListenerOverrides { host: None, port: Some(7777) })
            .expect("config should load")
            .config;
        pretty_assertions::assert_eq!(config.server.port, 7777);
    });
}

#[test]
#[serial]
fn a_single_prefix_underscore_name_is_not_read() {
    temp_env::with_vars([("ANDROMEDA_PORT", Some("8888"))], || {
        let config = load_env_only();
        pretty_assertions::assert_eq!(config.server.port, 1056);
    });
}

#[test]
#[serial]
fn a_single_underscore_between_table_and_key_is_ignored_as_an_unknown_key() {
    temp_env::with_vars([("ANDROMEDA__SERVER_HOST", Some("203.0.113.1"))], || {
        let result = load(None, ListenerOverrides::default());
        let loaded = result.expect("an unknown environment key loads");
        pretty_assertions::assert_eq!(ignored_settings_of(&loaded), vec![("server_host".to_owned(), "environment")]);
        pretty_assertions::assert_eq!(loaded.config.server.host, BrokerConfig::default().server.host);
    });
}

#[test]
fn default_precedence_summary_matches_contract() {
    pretty_assertions::assert_eq!(PRECEDENCE, "CLI host/port > environment > config file > defaults");
}

#[test]
fn an_explicit_missing_configuration_file_is_rejected() {
    let path = PathBuf::from("/nonexistent-andromeda-explicit-check.toml");
    let resolved = resolve_configuration_file(Some(path.clone()));
    assert!(matches!(&resolved, Err(LoadError::MissingFile(missing)) if *missing == path), "{resolved:?}");
}

#[test]
fn an_absent_default_configuration_file_is_not_an_error() {
    assert!(resolve_configuration_file(None).is_ok());
}

#[test]
fn the_default_configuration_file_lives_under_the_application_subdir() {
    // Skipped where no home directory is available (some CI containers), in
    // which case the broker runs from defaults and environment alone.
    if let Some(path) = default_configuration_file() {
        assert!(path.ends_with("andromeda/andromeda.toml"), "{}", path.display());
    }
}

#[test]
fn the_template_renders_tables_and_commented_fields() {
    let rendered = configuration_template().expect("the template renders");
    assert!(rendered.contains("[server]"), "{rendered}");
    assert!(rendered.contains("[database.maintenance]"), "{rendered}");
    assert!(rendered.lines().any(|line| line.starts_with('#')), "{rendered}");
}

/// History is written by one statement shape, so the configuration has no
/// bulk-write strategy to choose.
#[test]
fn the_default_configuration_names_no_atomic_write_table() {
    let rendered = configuration_template().expect("the template renders");
    assert!(!rendered.contains("atomic_write") && !rendered.contains("bulk_mode"), "{rendered}");
}

macro_rules! an_unknown_key_is_ignored_and_reported_with_its_path {
    ($($name:ident: $table:literal,)+) => {
        $(
            #[test]
            #[serial]
            fn $name() {
                pretty_assertions::assert_eq!(
                    ignored_in(concat!("[", $table, "]\nunknown_key = 1\n")),
                    in_file(concat!($table, ".unknown_key"))
                );
            }
        )+
    };
}

an_unknown_key_is_ignored_and_reported_with_its_path! {
    an_unknown_server_key_is_ignored: "server",
    an_unknown_server_http3_key_is_ignored: "server.http3",
    an_unknown_tls_key_is_ignored: "tls",
    an_unknown_tls_client_authentication_key_is_ignored: "tls.client_authentication",
    an_unknown_management_key_is_ignored: "management",
    an_unknown_request_limits_key_is_ignored: "request_limits",
    an_unknown_http_key_is_ignored: "http",
    an_unknown_http_body_decoding_key_is_ignored: "http.body_decoding",
    an_unknown_http_cors_key_is_ignored: "http.cors",
    an_unknown_http_compression_key_is_ignored: "http.compression",
    an_unknown_http_decompression_key_is_ignored: "http.decompression",
    an_unknown_http_server_timing_key_is_ignored: "http.server_timing",
    an_unknown_http_security_headers_key_is_ignored: "http.security_headers",
    an_unknown_http_csrf_key_is_ignored: "http.csrf",
    an_unknown_database_key_is_ignored: "database",
    an_unknown_database_name_cache_key_is_ignored: "database.name_cache",
    an_unknown_database_maintenance_key_is_ignored: "database.maintenance",
    an_unknown_database_maintenance_repack_key_is_ignored: "database.maintenance.repack",
    an_unknown_tenancy_key_is_ignored: "tenancy",
    an_unknown_tenancy_databases_key_is_ignored: "tenancy.databases",
    an_unknown_tenancy_default_tenant_key_is_ignored: "tenancy.default_tenant",
    an_unknown_tenancy_residency_key_is_ignored: "tenancy.residency",
    an_unknown_tenancy_build_key_is_ignored: "tenancy.build",
    an_unknown_expiry_key_is_ignored: "expiry",
    an_unknown_jsonld_key_is_ignored: "jsonld",
    an_unknown_jsonld_fetch_key_is_ignored: "jsonld.fetch",
    an_unknown_jsonld_cache_key_is_ignored: "jsonld.cache",
    an_unknown_entity_key_is_ignored: "entity",
    an_unknown_entity_batch_key_is_ignored: "entity.batch",
    an_unknown_entity_side_effects_key_is_ignored: "entity.side_effects",
    an_unknown_entity_side_effects_queue_key_is_ignored: "entity.side_effects.queue",
    an_unknown_entity_side_effects_retention_key_is_ignored: "entity.side_effects.retention",
    an_unknown_entity_spatial_index_key_is_ignored: "entity.spatial_index",
    an_unknown_temporal_key_is_ignored: "temporal",
    an_unknown_temporal_query_key_is_ignored: "temporal.query",
    an_unknown_temporal_history_key_is_ignored: "temporal.history",
    an_unknown_temporal_history_queue_key_is_ignored: "temporal.history.queue",
    an_unknown_temporal_storage_key_is_ignored: "temporal.storage",
    an_unknown_temporal_storage_columnstore_key_is_ignored: "temporal.storage.columnstore",
    an_unknown_temporal_storage_retention_key_is_ignored: "temporal.storage.retention",
    an_unknown_notifications_key_is_ignored: "notifications",
    an_unknown_notifications_delivery_key_is_ignored: "notifications.delivery",
    an_unknown_notifications_delivery_queue_key_is_ignored: "notifications.delivery.queue",
    an_unknown_notifications_delivery_retention_key_is_ignored: "notifications.delivery.retention",
    an_unknown_notifications_evaluation_key_is_ignored: "notifications.evaluation",
    an_unknown_notifications_periodic_key_is_ignored: "notifications.periodic",
    an_unknown_federation_key_is_ignored: "federation",
    an_unknown_federation_forwarding_key_is_ignored: "federation.forwarding",
    an_unknown_federation_query_key_is_ignored: "federation.query",
    an_unknown_federation_response_cache_key_is_ignored: "federation.response_cache",
    an_unknown_federation_cooldown_key_is_ignored: "federation.cooldown",
    an_unknown_federation_identity_key_is_ignored: "federation.identity",
    an_unknown_snapshot_key_is_ignored: "snapshot",
    an_unknown_snapshot_quota_key_is_ignored: "snapshot.quota",
    an_unknown_snapshot_preparation_key_is_ignored: "snapshot.preparation",
    an_unknown_extensions_key_is_ignored: "extensions",
    an_unknown_extensions_cursor_pagination_key_is_ignored: "extensions.cursor_pagination",
    an_unknown_extensions_temporal_delete_by_query_key_is_ignored: "extensions.temporal_delete_by_query",
    an_unknown_extensions_tenant_management_key_is_ignored: "extensions.tenant_management",
    an_unknown_extensions_q_target_languages_key_is_ignored: "extensions.q_target_languages",
    an_unknown_observability_key_is_ignored: "observability",
    an_unknown_observability_log_key_is_ignored: "observability.log",
    an_unknown_observability_log_file_key_is_ignored: "observability.log.file",
    an_unknown_observability_metrics_key_is_ignored: "observability.metrics",
    an_unknown_observability_metrics_prometheus_key_is_ignored: "observability.metrics.prometheus",
}

#[test]
#[serial]
fn an_unknown_database_pooler_key_is_ignored_and_the_pooler_still_loads() {
    let loaded = load_file_reporting("[database.pooler]\nurl = \"postgres://pooler/db\"\nmax_connections_per_tenant = 2\nunknown_key = 1\n")
        .expect("an unknown pooler key loads");
    pretty_assertions::assert_eq!(ignored_settings_of(&loaded), in_file("database.pooler.unknown_key"));
    assert!(loaded.config.database.is_pooled());
}

#[test]
#[serial]
fn an_unknown_key_in_a_preloaded_context_entry_is_ignored_with_the_entry_index() {
    let loaded = load_file_reporting(concat!(
        "[[jsonld.preloaded_contexts]]\nurl = \"https://example.com/a.jsonld\"\npath = \"a.jsonld\"\n",
        "[[jsonld.preloaded_contexts]]\nurl = \"https://example.com/b.jsonld\"\npath = \"b.jsonld\"\nunknown_key = 1\n",
    ))
    .expect("an unknown preloaded-context key loads");
    pretty_assertions::assert_eq!(ignored_settings_of(&loaded), in_file("jsonld.preloaded_contexts[1].unknown_key"));
    pretty_assertions::assert_eq!(loaded.config.jsonld.preloaded_contexts.len(), 2);
}

#[test]
#[serial]
fn an_unknown_top_level_key_is_ignored() {
    pretty_assertions::assert_eq!(ignored_in("unknown_section = 1\n"), in_file("unknown_section"));
}

/// A table the configuration does not define is reported once, as a whole,
/// not once for each key it holds.
#[test]
#[serial]
fn an_unknown_top_level_table_is_ignored_as_one_setting() {
    pretty_assertions::assert_eq!(ignored_in("[future_feature]\nmode = \"enabled\"\nlimit = 3\n"), in_file("future_feature"));
}

#[test]
#[serial]
fn an_unknown_nested_table_is_ignored_and_its_siblings_still_load() {
    let loaded = load_file_reporting("[server]\nport = 1600\n\n[server.future]\nmode = \"enabled\"\n").expect("an unknown nested table loads");
    pretty_assertions::assert_eq!(ignored_settings_of(&loaded), in_file("server.future"));
    pretty_assertions::assert_eq!(loaded.config.server.port, 1600);
}

#[test]
#[serial]
fn a_file_without_unknown_keys_ignores_nothing() {
    pretty_assertions::assert_eq!(ignored_in("[server]\nport = 1600\n"), Vec::new());
}

/// The message names the key, where it is written and that it is ignored.
#[test]
#[serial]
fn an_ignored_file_key_names_its_path_and_its_file() {
    let path = write_temp_config("[server]\nunknown_key = 1\n");
    let result = load(Some(&path), ListenerOverrides::default());
    fs::remove_file(&path).expect("temp config should be removed");
    let message = result
        .expect("an unknown key loads")
        .ignored
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    pretty_assertions::assert_eq!(
        message,
        vec![format!(
            "`server.unknown_key` in {} is not a setting this broker reads, so it is ignored",
            path.display()
        )]
    );
}

#[test]
#[serial]
fn an_unknown_prefixed_environment_variable_is_ignored_and_named() {
    temp_env::with_vars([("ANDROMEDA__HTTP__SERVER_TIMING__PHASEZ", Some("1"))], || {
        let result = load(None, ListenerOverrides::default());
        let loaded = result.expect("an unknown environment key loads");
        pretty_assertions::assert_eq!(ignored_settings_of(&loaded), vec![("http.server_timing.phasez".to_owned(), "environment")]);
        pretty_assertions::assert_eq!(
            loaded.ignored.iter().map(ToString::to_string).collect::<Vec<_>>(),
            vec![
                "ANDROMEDA__HTTP__SERVER_TIMING__PHASEZ sets `http.server_timing.phasez`, which is not a setting this broker reads, so it is ignored"
                    .to_owned()
            ]
        );
    });
}

/// The file and the environment each write the unknown key, so each is
/// reported.
#[test]
#[serial]
fn an_unknown_key_written_by_the_file_and_the_environment_is_reported_for_both() {
    let path = write_temp_config("[server]\nunknown_key = 1\n");
    temp_env::with_vars([("ANDROMEDA__SERVER__UNKNOWN_KEY", Some("2"))], || {
        let result = load(Some(&path), ListenerOverrides::default());
        fs::remove_file(&path).expect("temp config should be removed");
        pretty_assertions::assert_eq!(
            ignored_settings_of(&result.expect("an unknown key loads")),
            vec![("server.unknown_key".to_owned(), "file"), ("server.unknown_key".to_owned(), "environment")]
        );
    });
}

/// A key the configuration defines holding a value of the wrong type is
/// refused, never ignored.
#[test]
#[serial]
fn a_defined_key_with_a_value_of_the_wrong_type_is_still_refused() {
    let refused = load_file("[server]\nport = \"high\"\n");
    assert!(matches!(&refused, Err(LoadError::Layers(_))), "{:?}", refused.map(|_| ()));
}

#[test]
#[serial]
fn a_defined_duration_key_with_a_malformed_value_is_still_refused() {
    let refused = load_file("[server]\ngraceful_drain_timeout = \"soon\"\n");
    assert!(matches!(&refused, Err(LoadError::Layers(_))), "{:?}", refused.map(|_| ()));
}

#[test]
#[serial]
fn an_environment_variable_without_the_prefix_is_not_read() {
    temp_env::with_vars([("HTTP__SERVER_TIMING__PHASEZ", Some("1"))], || {
        let result = load(None, ListenerOverrides::default());
        result.expect("an unprefixed variable never reaches the configuration");
    });
}

#[test]
#[serial]
fn a_source_id_that_is_not_an_iri_is_refused_at_load() {
    let message = load_file("[federation.identity]\nsource_id = \"not an iri\"\n")
        .expect_err("a source_id that is not an IRI must fail loading")
        .to_string();
    assert!(message.contains("source_id is not an IRI"), "{message}");
}

#[test]
#[serial]
fn federation_forwarding_private_addresses_reads_refused_and_refuses_any_other_value() {
    let config = load_file("[federation.forwarding]\nprivate_addresses = \"refused\"\n").expect("refused is a defined value");
    assert_eq!(config.federation.forwarding.private_addresses, PrivateAddresses::Refused);
    load_file("[federation.forwarding]\nprivate_addresses = \"bogus\"\n").expect_err("bogus is not a defined value");
}

#[test]
#[serial]
fn the_address_lists_of_both_outbound_surfaces_read_mixed_entries_and_default_to_empty() {
    let defaults = load_file("").expect("an empty file loads");
    assert_eq!(defaults.notifications.delivery.allowed_addresses, Vec::new());
    assert_eq!(defaults.notifications.delivery.denied_addresses, Vec::new());
    assert_eq!(defaults.federation.forwarding.allowed_addresses, Vec::new());
    assert_eq!(defaults.federation.forwarding.denied_addresses, Vec::new());

    let config = load_file(concat!(
        "[notifications.delivery]\n",
        "private_addresses = \"refused\"\n",
        "allowed_addresses = [\"10.2.0.0/24\", \"10.3.0.17\", \"fd00:1234::/48\"]\n",
        "denied_addresses = [\"203.0.113.0/24\", \"198.51.100.9\"]\n",
        "[federation.forwarding]\n",
        "private_addresses = \"refused\"\n",
        "allowed_addresses = [\"10.8.0.0/16\"]\n",
    ))
    .expect("addresses and subnets, IPv4 and IPv6, are entries");
    assert_eq!(config.notifications.delivery.allowed_addresses.len(), 3);
    assert_eq!(config.notifications.delivery.denied_addresses.len(), 2);
    assert_eq!(config.federation.forwarding.allowed_addresses.len(), 1);
}

#[test]
#[serial]
fn an_address_list_entry_that_is_not_an_address_or_a_subnet_is_refused_with_its_key() {
    for (table, key) in [
        ("notifications.delivery", "allowed_addresses"),
        ("notifications.delivery", "denied_addresses"),
        ("federation.forwarding", "allowed_addresses"),
        ("federation.forwarding", "denied_addresses"),
    ] {
        for entry in ["example.org", "10.0.0.1:8080", "10.2.0.0/33", "not an address"] {
            let message = load_file(&format!("[{table}]\n{key} = [\"10.2.0.0/24\", \"{entry}\"]\n"))
                .expect_err("a bad entry must fail loading")
                .to_string();
            assert!(message.contains(key) && message.contains(entry), "{table}.{key} {entry}: {message}");
        }
    }
}

macro_rules! zero_batch_size_is_refused_with_its_path {
    ($($name:ident: $table:literal,)+) => {
        $(
            #[test]
            #[serial]
            fn $name() {
                let error = load_file(concat!("[", $table, "]\nbatch_size = 0\n")).expect_err("a zero batch size must fail loading");
                assert!(matches!(error, LoadError::Layers(_)), "{error:?}");
                let message = error.to_string();
                assert!(message.contains(concat!($table, ".batch_size")), "{message}");
                assert!(message.contains("nonzero"), "{message}");
            }
        )+
    };
}

zero_batch_size_is_refused_with_its_path! {
    a_zero_expiry_batch_size_is_refused: "expiry",
    a_zero_side_effect_batch_size_is_refused: "entity.side_effects",
    a_zero_temporal_history_batch_size_is_refused: "temporal.history",
}
