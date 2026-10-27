//! Loading the layered configuration: defaults, file, environment and flags.

mod key_tree;

use crate::{
    broker_config::BrokerConfig,
    load::{ListenerOverrides, LoadError, PRECEDENCE, default_config_file, load, resolve_config_file},
    redact::{redacted_json, redacted_toml},
    schema::{
        content_coding::ContentCoding,
        database::{pooler::PoolerConfig, write_reserve::WriteReserve},
        entity::spatial_index::SpatialConsistency,
        http::{
            compression::CompressionQuality,
            cors::{CorsMode, MethodRule},
        },
    },
    template::default_template,
};
use serial_test::serial;
use std::{
    env,
    fs,
    net::IpAddr,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

/// Load with only the built-in defaults and the environment, so no file layer
/// contributes.
fn load_env_only() -> BrokerConfig {
    load(None, ListenerOverrides::default()).expect("config should load")
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

/// Load `contents` as the configuration file.
fn load_file(contents: &str) -> Result<BrokerConfig, LoadError> {
    let path = write_temp_config(contents);
    let result = load(Some(&path), ListenerOverrides::default());
    fs::remove_file(path).expect("temp config should be removed");
    result
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
fn env_overrides_database_url() {
    unsafe { env::set_var("ANDROMEDA__DATABASE__URL", "postgres://envhost/envdb") };
    let config = load_env_only();
    unsafe { env::remove_var("ANDROMEDA__DATABASE__URL") };
    pretty_assertions::assert_eq!(config.database.url.as_str(), "postgres://envhost/envdb");
}

#[test]
#[serial]
fn env_reaches_a_key_three_tables_deep() {
    unsafe { env::set_var("ANDROMEDA__ENTITY__SIDE_EFFECTS__QUEUE__BATCH_SIZE", "64") };
    let config = load_env_only();
    unsafe { env::remove_var("ANDROMEDA__ENTITY__SIDE_EFFECTS__QUEUE__BATCH_SIZE") };
    pretty_assertions::assert_eq!(config.entity.side_effects.queue.batch_size.get(), 64);
}

#[test]
#[serial]
fn env_parses_enumerated_values_in_kebab_case() {
    unsafe {
        env::set_var("ANDROMEDA__HTTP__COMPRESSION__QUALITY", "best");
        env::set_var("ANDROMEDA__HTTP__CORS__MODE", "disabled");
        env::set_var("ANDROMEDA__ENTITY__SPATIAL_INDEX__CONSISTENCY", "store-verified");
    };
    let config = load_env_only();
    unsafe {
        env::remove_var("ANDROMEDA__HTTP__COMPRESSION__QUALITY");
        env::remove_var("ANDROMEDA__HTTP__CORS__MODE");
        env::remove_var("ANDROMEDA__ENTITY__SPATIAL_INDEX__CONSISTENCY");
    };
    pretty_assertions::assert_eq!(config.http.compression.quality, CompressionQuality::Best);
    pretty_assertions::assert_eq!(config.http.cors.mode, CorsMode::Disabled);
    pretty_assertions::assert_eq!(config.entity.spatial_index.consistency, SpatialConsistency::StoreVerified);
}

/// A table is set in the file alone: an environment value is a scalar, so
/// JSON naming a table is text, which the table setting refuses.
#[test]
#[serial]
fn a_table_cannot_be_set_from_the_environment() {
    unsafe {
        env::set_var(
            "ANDROMEDA__JSONLD__PRELOADED_CONTEXTS",
            "[{\"url\":\"https://example.com/context.jsonld\",\"path\":\"contexts/test.jsonld\"}]",
        );
    };
    let result = load(None, ListenerOverrides::default());
    unsafe { env::remove_var("ANDROMEDA__JSONLD__PRELOADED_CONTEXTS") };
    assert!(matches!(result, Err(LoadError::Layers(_))), "{result:?}");
}

#[test]
#[serial]
fn a_list_set_from_the_environment_splits_on_commas() {
    unsafe { env::set_var("ANDROMEDA__HTTP__COMPRESSION__ALGORITHMS", "gzip,zstd") };
    let config = load_env_only();
    unsafe { env::remove_var("ANDROMEDA__HTTP__COMPRESSION__ALGORITHMS") };
    pretty_assertions::assert_eq!(config.http.compression.algorithms, ContentCoding::Gzip | ContentCoding::Zstd);
}

#[test]
#[serial]
fn a_cors_method_list_set_from_the_environment_splits_on_commas() {
    unsafe {
        env::set_var("ANDROMEDA__HTTP__CORS__MODE", "enabled");
        env::set_var("ANDROMEDA__HTTP__CORS__ALLOW_METHODS", "GET,POST");
    };
    let config = load_env_only();
    unsafe {
        env::remove_var("ANDROMEDA__HTTP__CORS__MODE");
        env::remove_var("ANDROMEDA__HTTP__CORS__ALLOW_METHODS");
    };
    assert!(
        matches!(&config.http.cors.allow_methods, MethodRule::List(methods) if methods.len() == 2),
        "{:?}",
        config.http.cors.allow_methods
    );
}

/// The defaults written by `configuration generate` load back to the
/// defaults.
#[test]
#[serial]
fn the_generated_defaults_load_back_to_the_defaults() {
    let loaded = load_file(&default_template()).expect("the generated defaults load");
    pretty_assertions::assert_eq!(
        toml::to_string(&loaded).expect("the loaded configuration renders"),
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

/// The delivery lease must leave its margin over one attempt's request
/// timeout: one second short of it refuses to load, the margin exactly
/// loads.
#[test]
#[serial]
fn a_delivery_lease_is_refused_below_its_margin_over_the_request_timeout_and_loads_at_it() {
    let refused = load_file("[notifications.delivery]\nrequest_timeout = \"10s\"\n[notifications.delivery.queue]\nlease_timeout = \"14s\"\n");
    let loaded = load_file("[notifications.delivery]\nrequest_timeout = \"10s\"\n[notifications.delivery.queue]\nlease_timeout = \"15s\"\n");
    let error = refused.expect_err("a 14s lease over a 10s request timeout lacks the margin");
    assert!(
        error
            .to_string()
            .contains("notifications.delivery.queue.lease_timeout must exceed notifications.delivery.request_timeout by at least 5s"),
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
    let refused = load_file("[database]\nacquire_timeout = \"5s\"\n[entity.side_effects.queue]\nlease_timeout = \"9s\"\n");
    let loaded = load_file("[database]\nacquire_timeout = \"5s\"\n[entity.side_effects.queue]\nlease_timeout = \"10s\"\n");
    let error = refused.expect_err("a 9s side-effect lease over a 5s acquire timeout lacks the margin");
    assert!(
        error
            .to_string()
            .contains("entity.side_effects.queue.lease_timeout (9s) must exceed database.acquire_timeout (5s) by at least 5s"),
        "{error}"
    );
    loaded.expect("a 10s side-effect lease over a 5s acquire timeout carries the margin");
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
    unsafe { env::set_var("ANDROMEDA__SERVER__PORT", "1700") };
    let config = load(Some(&path), ListenerOverrides::default()).expect("config should load");
    unsafe { env::remove_var("ANDROMEDA__SERVER__PORT") };
    fs::remove_file(path).expect("temp config should be removed");
    pretty_assertions::assert_eq!(config.server.port, 1700);
}

#[test]
#[serial]
fn cli_overrides_env() {
    unsafe { env::set_var("ANDROMEDA__SERVER__PORT", "8888") };
    let config = load(None, ListenerOverrides { host: None, port: Some(7777) }).expect("config should load");
    unsafe { env::remove_var("ANDROMEDA__SERVER__PORT") };
    pretty_assertions::assert_eq!(config.server.port, 7777);
}

#[test]
#[serial]
fn a_single_prefix_underscore_name_is_not_read() {
    unsafe { env::set_var("ANDROMEDA_PORT", "8888") };
    let config = load_env_only();
    unsafe { env::remove_var("ANDROMEDA_PORT") };
    pretty_assertions::assert_eq!(config.server.port, 1056);
}

#[test]
#[serial]
fn a_single_underscore_between_table_and_key_is_refused_as_an_unknown_table() {
    unsafe { env::set_var("ANDROMEDA__SERVER_HOST", "203.0.113.1") };
    let result = load(None, ListenerOverrides::default());
    unsafe { env::remove_var("ANDROMEDA__SERVER_HOST") };
    let message = result.expect_err("a table named server_host does not exist").to_string();
    assert!(message.to_lowercase().contains("server_host"), "{message}");
}

#[test]
fn default_precedence_summary_matches_contract() {
    pretty_assertions::assert_eq!(PRECEDENCE, "CLI host/port > environment > config file > defaults");
}

#[test]
fn an_explicit_missing_config_file_is_rejected() {
    let path = PathBuf::from("/nonexistent-andromeda-explicit-check.toml");
    let resolved = resolve_config_file(Some(path.clone()));
    assert!(matches!(&resolved, Err(LoadError::MissingFile(missing)) if *missing == path), "{resolved:?}");
}

#[test]
fn an_absent_default_config_file_is_not_an_error() {
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
fn the_template_renders_tables_and_commented_fields() {
    let rendered = default_template();
    assert!(rendered.contains("[server]"), "{rendered}");
    assert!(rendered.contains("[database.maintenance]"), "{rendered}");
    assert!(rendered.lines().any(|line| line.starts_with('#')), "{rendered}");
}

/// History is written by one statement shape, so the configuration has no
/// bulk-write strategy to choose.
#[test]
fn the_default_configuration_names_no_atomic_write_table() {
    let rendered = default_template();
    assert!(!rendered.contains("atomic_write") && !rendered.contains("bulk_mode"), "{rendered}");
}

/// Load a file holding one misspelled key under `table` and return the
/// rendered startup error.
fn unknown_key_error(table: &str) -> String {
    let error = load_file(&format!("[{table}]\nunknown_key = 1\n")).expect_err("an unknown key must fail loading");
    assert!(matches!(error, LoadError::Layers(_)), "{error:?}");
    error.to_string()
}

macro_rules! unknown_key_is_refused_with_its_path {
    ($($name:ident: $table:literal,)+) => {
        $(
            #[test]
            #[serial]
            fn $name() {
                let message = unknown_key_error($table);
                assert!(message.contains(concat!($table, ".unknown_key")), "{message}");
                assert!(message.contains("unknown field: found `unknown_key`, expected"), "{message}");
            }
        )+
    };
}

unknown_key_is_refused_with_its_path! {
    an_unknown_server_key_is_refused: "server",
    an_unknown_server_http3_key_is_refused: "server.http3",
    an_unknown_tls_key_is_refused: "tls",
    an_unknown_tls_client_authentication_key_is_refused: "tls.client_authentication",
    an_unknown_management_key_is_refused: "management",
    an_unknown_request_limits_key_is_refused: "request_limits",
    an_unknown_http_key_is_refused: "http",
    an_unknown_http_body_decoding_key_is_refused: "http.body_decoding",
    an_unknown_http_cors_key_is_refused: "http.cors",
    an_unknown_http_compression_key_is_refused: "http.compression",
    an_unknown_http_decompression_key_is_refused: "http.decompression",
    an_unknown_http_server_timing_key_is_refused: "http.server_timing",
    an_unknown_http_security_headers_key_is_refused: "http.security_headers",
    an_unknown_http_csrf_key_is_refused: "http.csrf",
    an_unknown_database_key_is_refused: "database",
    an_unknown_database_name_cache_key_is_refused: "database.name_cache",
    an_unknown_database_maintenance_key_is_refused: "database.maintenance",
    an_unknown_tenancy_key_is_refused: "tenancy",
    an_unknown_tenancy_databases_key_is_refused: "tenancy.databases",
    an_unknown_tenancy_residency_key_is_refused: "tenancy.residency",
    an_unknown_tenancy_build_key_is_refused: "tenancy.build",
    an_unknown_expiry_key_is_refused: "expiry",
    an_unknown_jsonld_key_is_refused: "jsonld",
    an_unknown_jsonld_fetch_key_is_refused: "jsonld.fetch",
    an_unknown_jsonld_cache_key_is_refused: "jsonld.cache",
    an_unknown_entity_key_is_refused: "entity",
    an_unknown_entity_batch_key_is_refused: "entity.batch",
    an_unknown_entity_side_effects_key_is_refused: "entity.side_effects",
    an_unknown_entity_side_effects_queue_key_is_refused: "entity.side_effects.queue",
    an_unknown_entity_side_effects_retention_key_is_refused: "entity.side_effects.retention",
    an_unknown_entity_spatial_index_key_is_refused: "entity.spatial_index",
    an_unknown_temporal_key_is_refused: "temporal",
    an_unknown_temporal_query_key_is_refused: "temporal.query",
    an_unknown_temporal_history_key_is_refused: "temporal.history",
    an_unknown_temporal_history_queue_key_is_refused: "temporal.history.queue",
    an_unknown_temporal_storage_key_is_refused: "temporal.storage",
    an_unknown_temporal_storage_columnstore_key_is_refused: "temporal.storage.columnstore",
    an_unknown_temporal_storage_retention_key_is_refused: "temporal.storage.retention",
    an_unknown_notifications_key_is_refused: "notifications",
    an_unknown_notifications_delivery_key_is_refused: "notifications.delivery",
    an_unknown_notifications_delivery_queue_key_is_refused: "notifications.delivery.queue",
    an_unknown_notifications_delivery_retention_key_is_refused: "notifications.delivery.retention",
    an_unknown_notifications_evaluation_key_is_refused: "notifications.evaluation",
    an_unknown_notifications_periodic_key_is_refused: "notifications.periodic",
    an_unknown_federation_key_is_refused: "federation",
    an_unknown_federation_forwarding_key_is_refused: "federation.forwarding",
    an_unknown_federation_response_cache_key_is_refused: "federation.response_cache",
    an_unknown_federation_cooldown_key_is_refused: "federation.cooldown",
    an_unknown_federation_identity_key_is_refused: "federation.identity",
    an_unknown_snapshot_key_is_refused: "snapshot",
    an_unknown_snapshot_quota_key_is_refused: "snapshot.quota",
    an_unknown_snapshot_preparation_key_is_refused: "snapshot.preparation",
    an_unknown_extensions_key_is_refused: "extensions",
    an_unknown_extensions_cursor_pagination_key_is_refused: "extensions.cursor_pagination",
    an_unknown_extensions_temporal_delete_by_query_key_is_refused: "extensions.temporal_delete_by_query",
    an_unknown_extensions_tenant_management_key_is_refused: "extensions.tenant_management",
    an_unknown_extensions_q_target_languages_key_is_refused: "extensions.q_target_languages",
    an_unknown_observability_key_is_refused: "observability",
    an_unknown_observability_log_key_is_refused: "observability.log",
    an_unknown_observability_log_file_key_is_refused: "observability.log.file",
    an_unknown_observability_metrics_key_is_refused: "observability.metrics",
}

#[test]
#[serial]
fn an_unknown_database_pooler_key_is_refused() {
    let message = load_file("[database.pooler]\nurl = \"postgres://pooler/db\"\nunknown_key = 1\n")
        .expect_err("an unknown pooler key must fail loading")
        .to_string();
    assert!(message.contains("database.pooler.unknown_key"), "{message}");
}

#[test]
#[serial]
fn an_unknown_preloaded_context_key_is_refused() {
    let message = load_file("[[jsonld.preloaded_contexts]]\nurl = \"https://example.com/c.jsonld\"\npath = \"c.jsonld\"\nunknown_key = 1\n")
        .expect_err("an unknown preloaded-context key must fail loading")
        .to_string();
    assert!(message.contains("unknown_key"), "{message}");
}

#[test]
#[serial]
fn an_unknown_top_level_key_is_refused() {
    let message = load_file("unknown_section = 1\n")
        .expect_err("an unknown top-level key must fail loading")
        .to_string();
    assert!(message.contains("unknown_section"), "{message}");
}

#[test]
#[serial]
fn an_unknown_prefixed_environment_variable_is_refused() {
    unsafe { env::set_var("ANDROMEDA__HTTP__SERVER_TIMING__PHASEZ", "1") };
    let result = load(None, ListenerOverrides::default());
    unsafe { env::remove_var("ANDROMEDA__HTTP__SERVER_TIMING__PHASEZ") };
    let message = result.expect_err("an unknown environment key must fail loading").to_string();
    assert!(message.to_lowercase().contains("http.server_timing.phasez"), "{message}");
}

#[test]
#[serial]
fn an_environment_variable_without_the_prefix_is_not_read() {
    unsafe { env::set_var("HTTP__SERVER_TIMING__PHASEZ", "1") };
    let result = load(None, ListenerOverrides::default());
    unsafe { env::remove_var("HTTP__SERVER_TIMING__PHASEZ") };
    result.expect("an unprefixed variable never reaches the configuration");
}

#[test]
#[serial]
fn a_source_id_that_is_not_an_iri_is_refused_at_load() {
    let message = load_file("[federation.identity]\nsource_id = \"not an iri\"\n")
        .expect_err("a source_id that is not an IRI must fail loading")
        .to_string();
    assert!(message.contains("source_id is not an IRI"), "{message}");
}
