//! Loading the layered configuration: defaults, file, environment and flags.

use crate::{
    broker_config::BrokerConfig,
    human_duration::HumanDuration,
    load::{ListenerOverrides, LoadError, PRECEDENCE, default_config_file, load, resolve_config_file},
    redact::{redacted_json, redacted_toml},
    schema::{
        content_coding::ContentCoding,
        database::PoolerConfig,
        http::{
            compression::CompressionQuality,
            cors::{CorsMode, MethodRule},
        },
        paging_mode::PagingMode,
        spatial::SpatialConsistency,
        write_reserve::WriteReserve,
    },
    template::default_template,
    validate::validate,
};
use serial_test::serial;
use std::{
    env,
    fs,
    net::IpAddr,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

/// A configuration built from the defaults with one preload context, for the
/// path-normalization, redaction and validation tests that vary one field.
fn config_with_preload(path: &str) -> BrokerConfig {
    let mut config = BrokerConfig::default();
    config
        .jsonld
        .preload_contexts
        .insert("https://example.com/context.jsonld".parse().expect("test URL"), path.into());
    config
}

/// Load with only the built-in defaults and the environment, against a path
/// that does not exist so no file layer contributes.
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

#[test]
fn normalize_preload_paths_against_config_file_dir() {
    let mut config = config_with_preload("contexts/test.jsonld");
    config.jsonld.resolve_preload_paths(Path::new("/tmp/andromeda/config"));
    let path = config
        .jsonld
        .preload_contexts
        .get(&"https://example.com/context.jsonld".parse().expect("test URL"))
        .expect("preload path missing");
    pretty_assertions::assert_eq!(path, "/tmp/andromeda/config/contexts/test.jsonld");
}

#[test]
fn normalize_preload_paths_keeps_absolute_path() {
    let mut config = config_with_preload("/opt/contexts/test.jsonld");
    config.jsonld.resolve_preload_paths(Path::new("/tmp/andromeda/config"));
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

/// A table is set in the file alone: an environment value is a scalar,
/// so JSON naming a map is text, which the map setting refuses.
#[test]
#[serial]
fn a_table_cannot_be_set_from_the_environment() {
    unsafe {
        env::set_var(
            "ANDROMEDA__JSONLD__PRELOAD_CONTEXTS",
            "{\"https://example.com/context.jsonld\":\"contexts/test.jsonld\"}",
        );
    };
    let result = load(None, ListenerOverrides::default());
    unsafe { env::remove_var("ANDROMEDA__JSONLD__PRELOAD_CONTEXTS") };
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
    let path = write_temp_config(&default_template());
    let loaded = load(Some(&path), ListenerOverrides::default()).expect("the generated defaults load");
    fs::remove_file(path).expect("temp config should be removed");
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
    let json = redacted_json(&config).expect("redacted json should render");
    assert!(!json.contains("direct-secret") && !json.contains("pooler-secret"), "{json}");
}

#[test]
#[serial]
fn file_overrides_defaults() {
    let path = write_temp_config(
        r#"
[server]
port = 1600

[notifications]
interval_max_sleep = "45s"
"#,
    );

    let config = load(Some(&path), ListenerOverrides::default()).expect("config should load");
    fs::remove_file(path).expect("temp config should be removed");

    pretty_assertions::assert_eq!(config.server.port, 1600);
    pretty_assertions::assert_eq!(config.notifications.interval_max_sleep.duration().as_secs(), 45);
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
        let config = load(Some(&path), ListenerOverrides::default()).expect("maintenance config should load");
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
        let result = load(Some(&path), ListenerOverrides::default());
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

    let result = load(Some(&path), ListenerOverrides::default());
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
    let refused = load(Some(&short), ListenerOverrides::default());
    fs::remove_file(short).expect("temp config should be removed");
    let exact = write_temp_config("[notifications.delivery]\nrequest_timeout = \"10s\"\nlease = \"15s\"\n");
    let loaded = load(Some(&exact), ListenerOverrides::default());
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
    let refused = load(Some(&short), ListenerOverrides::default());
    fs::remove_file(short).expect("temp config should be removed");
    let exact = write_temp_config("[database]\nacquire_timeout = \"5s\"\n[entity]\npending_side_effects_lease = \"10s\"\n");
    let loaded = load(Some(&exact), ListenerOverrides::default());
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
    let loaded = load(Some(&set), ListenerOverrides::default());
    fs::remove_file(set).expect("temp config should be removed");
    let whole = write_temp_config("[database]\nwrite_reserve_percent = 100\n");
    let refused = load(Some(&whole), ListenerOverrides::default());
    fs::remove_file(whole).expect("temp config should be removed");

    let loaded = loaded.expect("a 40 percent reserve loads");
    assert_eq!(loaded.database.write_reserve_percent.read_connections(10), 6);
    assert_eq!(BrokerConfig::default().database.write_reserve_percent, WriteReserve::DEFAULT);
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
fn a_single_underscore_between_section_and_key_is_refused_as_an_unknown_section() {
    unsafe { env::set_var("ANDROMEDA__SERVER_HOST", "203.0.113.1") };
    let result = load(None, ListenerOverrides::default());
    unsafe { env::remove_var("ANDROMEDA__SERVER_HOST") };

    let message = result.expect_err("a section named server_host does not exist").to_string();
    assert!(message.to_lowercase().contains("server_host"), "{message}");
}

#[test]
fn redacted_effective_config_hides_secrets() {
    let mut config = config_with_preload("contexts/test.jsonld");
    config.database.url = "postgres://secret".parse().expect("database URL");

    let rendered = redacted_toml(&config).expect("redacted toml should render");

    assert!(!rendered.contains("postgres://secret"));
    assert!(rendered.contains("redacted.invalid"));
    assert!(rendered.contains("[database.maintenance]"));
    assert!(rendered.contains("mode = \"disabled\""));
    assert!(rendered.contains("profile = \"conservative\""));
}

#[test]
fn validate_rejects_zero_spatial_rebuild_interval() {
    let mut config = config_with_preload("contexts/test.jsonld");
    config.spatial.rebuild_interval = HumanDuration::from_secs(0);

    assert!(validate(&config).is_err());
}

#[test]
fn validate_rejects_zero_live_change_capacity() {
    let mut config = config_with_preload("contexts/test.jsonld");
    config.notifications.live_change_capacity = 0;

    assert!(validate(&config).is_err());
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
    let mut config = config_with_preload("contexts/test.jsonld");
    config.http.request_timeout = HumanDuration::from_secs(0);

    assert!(validate(&config).is_err());
}

#[test]
fn validate_rejects_zero_batch_chunk_write_concurrency() {
    let mut config = config_with_preload("contexts/test.jsonld");
    config.entity.batch_chunk_write_concurrency = Some(0);

    assert!(validate(&config).is_err());
}

#[test]
fn validate_rejects_zero_entity_expiry_batch_size() {
    let mut config = config_with_preload("contexts/test.jsonld");
    config.entity.expiry_batch_size = 0;

    assert!(validate(&config).is_err());
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
    let rendered = default_template();
    assert!(rendered.contains("[server]"), "{rendered}");
    assert!(rendered.contains("[database.maintenance]"), "{rendered}");
    assert!(rendered.lines().any(|line| line.starts_with('#')), "{rendered}");
}

/// History is written by one statement shape, so the configuration has no
/// bulk-write strategy to choose.
#[test]
fn the_default_configuration_names_no_atomic_write_section() {
    let rendered = default_template();
    assert!(!rendered.contains("atomic_write") && !rendered.contains("bulk_mode"), "{rendered}");
}

#[test]
fn default_toml_contains_the_management_table() {
    let rendered = default_template();
    assert!(rendered.contains("[management]"), "{rendered}");
    assert!(rendered.contains("mode = \"separate-port\""), "{rendered}");
    assert!(rendered.contains("port = 1086"), "{rendered}");
    assert!(rendered.contains("metrics = \"disabled\""), "{rendered}");
}

#[test]
fn generated_config_renders_the_pagination_section() {
    let rendered = default_template();
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

/// Load a file holding one misspelled key under `table` and return the
/// rendered startup error.
fn unknown_key_error(table: &str) -> String {
    let path = write_temp_config(&format!("[{table}]\nunknown_key = 1\n"));
    let result = load(Some(&path), ListenerOverrides::default());
    fs::remove_file(path).expect("temp config should be removed");
    let error = result.expect_err("an unknown key must fail loading");
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
    an_unknown_http_key_is_refused: "http",
    an_unknown_http_cors_key_is_refused: "http.cors",
    an_unknown_http_compression_key_is_refused: "http.compression",
    an_unknown_http_decompression_key_is_refused: "http.decompression",
    an_unknown_http_server_timing_key_is_refused: "http.server_timing",
    an_unknown_http_helmet_key_is_refused: "http.helmet",
    an_unknown_http_csrf_key_is_refused: "http.csrf",
    an_unknown_http3_key_is_refused: "http3",
    an_unknown_tls_key_is_refused: "tls",
    an_unknown_database_key_is_refused: "database",
    an_unknown_database_name_cache_key_is_refused: "database.name_cache",
    an_unknown_database_maintenance_key_is_refused: "database.maintenance",
    an_unknown_jsonld_key_is_refused: "jsonld",
    an_unknown_temporal_key_is_refused: "temporal",
    an_unknown_tenancy_key_is_refused: "tenancy",
    an_unknown_identity_key_is_refused: "identity",
    an_unknown_entity_key_is_refused: "entity",
    an_unknown_federation_key_is_refused: "federation",
    an_unknown_snapshot_key_is_refused: "snapshot",
    an_unknown_notifications_key_is_refused: "notifications",
    an_unknown_notifications_delivery_key_is_refused: "notifications.delivery",
    an_unknown_spatial_key_is_refused: "spatial",
    an_unknown_observability_key_is_refused: "observability",
    an_unknown_observability_file_logging_key_is_refused: "observability.file_logging",
    an_unknown_management_key_is_refused: "management",
    an_unknown_extensions_key_is_refused: "extensions",
    an_unknown_pagination_key_is_refused: "pagination",
}

#[test]
#[serial]
fn an_unknown_database_pooler_key_is_refused() {
    let path = write_temp_config("[database.pooler]\nurl = \"postgres://pooler/db\"\nunknown_key = 1\n");
    let result = load(Some(&path), ListenerOverrides::default());
    fs::remove_file(path).expect("temp config should be removed");
    let message = result.expect_err("an unknown pooler key must fail loading").to_string();
    assert!(message.contains("database.pooler.unknown_key"), "{message}");
}

#[test]
#[serial]
fn an_unknown_top_level_key_is_refused() {
    let path = write_temp_config("unknown_section = 1\n");
    let result = load(Some(&path), ListenerOverrides::default());
    fs::remove_file(path).expect("temp config should be removed");
    let message = result.expect_err("an unknown top-level key must fail loading").to_string();
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
    let path = write_temp_config("[identity]\nsource_id = \"not an iri\"\n");
    let result = load(Some(&path), ListenerOverrides::default());
    fs::remove_file(path).expect("temp config should be removed");
    let message = result.expect_err("a source_id that is not an IRI must fail loading").to_string();
    assert!(message.contains("identity.source_id is not an IRI"), "{message}");
}
