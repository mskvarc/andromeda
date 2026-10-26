//! Section admission: a value a domain refuses fails loading, and the
//! pagination policy the two shaping sections resolve to.

use crate::config::{
    commands::load_admitted,
    error::ConfigurationError,
    section_admission::{SectionAdmissionError, admit_sections, resolve_pagination},
};
use andromeda_configuration::{
    broker_config::BrokerConfig,
    load::ListenerOverrides,
    schema::{feature_mode::HttpFeatureMode, paging_mode::PagingMode},
};
use andromeda_ngsi_ld_request::pagination::policy::{PaginationPolicy, PaginationPolicyError};
use pretty_assertions::assert_eq;
use serial_test::serial;
use std::{
    env,
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

fn write_temp_config(contents: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after unix epoch")
        .as_nanos();
    let path = env::temp_dir().join(format!("andromeda-admission-{unique}.toml"));
    fs::write(&path, contents).expect("temp config should be written");
    path
}

fn resolved(config: &BrokerConfig) -> Result<PaginationPolicy, PaginationPolicyError> {
    resolve_pagination(&config.extensions, config.pagination)
}

/// A broker with neither section walks its collections by cursor.
#[test]
fn a_default_config_resolves_to_cursor_default() {
    assert_eq!(resolved(&BrokerConfig::default()), Ok(PaginationPolicy::CursorDefault));
}

/// Disabling cursors while keeping them the default is a contradiction the
/// configuration load refuses.
#[test]
fn disabled_cursor_with_cursor_default_fails_admission() {
    let mut config = BrokerConfig::default();
    config.extensions.cursor_pagination = HttpFeatureMode::Disabled;
    let error = admit_sections(&config).expect_err("the pair is refused");
    assert!(
        matches!(
            error,
            SectionAdmissionError::Pagination(PaginationPolicyError::CursorDefaultWithoutCursorPagination)
        ),
        "{error}"
    );
}

/// Disabling cursors with an offset default leaves offsets alone.
#[test]
fn disabled_cursor_with_offset_default_resolves_offset_only() {
    let mut config = BrokerConfig::default();
    config.extensions.cursor_pagination = HttpFeatureMode::Disabled;
    config.pagination.default_mode = PagingMode::Offset;
    assert_eq!(resolved(&config), Ok(PaginationPolicy::OffsetOnly));
    assert!(admit_sections(&config).is_ok());
}

/// The conformance override forces cursors off without the operator having
/// contradicted themselves, so the default falls back to offsets.
#[test]
fn conformance_mode_with_cursor_default_resolves_offset_only() {
    let mut config = BrokerConfig::default();
    config.extensions.conformance_mode = HttpFeatureMode::Enabled;
    assert_eq!(resolved(&config), Ok(PaginationPolicy::OffsetOnly));
    assert!(admit_sections(&config).is_ok());
}

/// An offset default with cursors on keeps the cursor walk available.
#[test]
fn an_offset_default_with_cursors_on_resolves_offset_default() {
    let mut config = BrokerConfig::default();
    config.pagination.default_mode = PagingMode::Offset;
    assert_eq!(resolved(&config), Ok(PaginationPolicy::OffsetDefault));
}

#[test]
fn the_default_configuration_is_admitted() {
    assert!(admit_sections(&BrokerConfig::default()).is_ok());
}

/// The identifier bound is the lexicon's, applied while loading, so an
/// over-long IRI never reaches the source identity route.
#[test]
#[serial]
fn a_source_id_past_the_identifier_bound_is_refused_at_load() {
    let path = write_temp_config(&format!("[identity]\nsource_id = \"urn:ngsi-ld:Node:{}\"\n", "a".repeat(1024)));
    let result = load_admitted(Some(path.clone()), ListenerOverrides::default()); // clone: the file is removed after loading
    fs::remove_file(path).expect("temp config should be removed");
    assert!(
        matches!(result, Err(ConfigurationError::Admission(SectionAdmissionError::SourceIdentifier(_)))),
        "{result:?}"
    );
}

#[test]
#[serial]
fn an_attribute_nesting_past_the_decodable_ceiling_is_refused_at_load() {
    let path = write_temp_config("[entity]\nmax_attribute_nesting = 63\n");
    let result = load_admitted(Some(path.clone()), ListenerOverrides::default()); // clone: the file is removed after loading
    fs::remove_file(path).expect("temp config should be removed");
    assert!(
        matches!(result, Err(ConfigurationError::Admission(SectionAdmissionError::EntityAttributeNesting(_)))),
        "{result:?}"
    );
}

#[test]
#[serial]
fn a_zero_side_effect_backoff_is_refused_at_load() {
    let path = write_temp_config("[entity]\npending_side_effects_base_backoff = \"0s\"\n");
    let result = load_admitted(Some(path.clone()), ListenerOverrides::default()); // clone: the file is removed after loading
    fs::remove_file(path).expect("temp config should be removed");
    assert!(
        matches!(result, Err(ConfigurationError::Admission(SectionAdmissionError::EntitySideEffectRetry(_)))),
        "{result:?}"
    );
}

#[test]
#[serial]
fn a_zero_delivery_concurrency_is_refused_at_load() {
    let path = write_temp_config("[notifications.delivery]\nconcurrency = 0\n");
    let result = load_admitted(Some(path.clone()), ListenerOverrides::default()); // clone: the file is removed after loading
    fs::remove_file(path).expect("temp config should be removed");
    assert!(
        matches!(result, Err(ConfigurationError::Admission(SectionAdmissionError::NotificationDelivery(_)))),
        "{result:?}"
    );
}

#[test]
#[serial]
fn a_zero_federation_cooldown_window_is_refused_at_load() {
    let path = write_temp_config("[federation]\ncooldown_mode = \"constant\"\ncooldown_window = \"0s\"\n");
    let result = load_admitted(Some(path.clone()), ListenerOverrides::default()); // clone: the file is removed after loading
    fs::remove_file(path).expect("temp config should be removed");
    assert!(
        matches!(result, Err(ConfigurationError::Admission(SectionAdmissionError::FederationCooldown(_)))),
        "{result:?}"
    );
}

#[test]
#[serial]
fn a_temporal_retry_ceiling_below_its_delay_is_refused_at_load() {
    let path = write_temp_config("[temporal]\nintent_retry = \"10s\"\nintent_retry_max = \"5s\"\n");
    let result = load_admitted(Some(path.clone()), ListenerOverrides::default()); // clone: the file is removed after loading
    fs::remove_file(path).expect("temp config should be removed");
    assert!(
        matches!(result, Err(ConfigurationError::Admission(SectionAdmissionError::TemporalIntentRetry(_)))),
        "{result:?}"
    );
}

#[test]
#[serial]
fn source_extras_holding_a_date_time_are_refused_at_load() {
    let path = write_temp_config("[identity.source_extras]\nsince = 1979-05-27\n");
    let result = load_admitted(Some(path.clone()), ListenerOverrides::default()); // clone: the file is removed after loading
    fs::remove_file(path).expect("temp config should be removed");
    assert!(
        matches!(result, Err(ConfigurationError::Admission(SectionAdmissionError::SourceExtras(_)))),
        "{result:?}"
    );
}

#[test]
#[serial]
fn a_cors_method_that_is_not_a_method_token_is_refused_at_load() {
    let path = write_temp_config("[http.cors]\nmode = \"enabled\"\nallow_methods = [\"G ET\"]\n");
    let result = load_admitted(Some(path.clone()), ListenerOverrides::default()); // clone: the file is removed after loading
    fs::remove_file(path).expect("temp config should be removed");
    assert!(
        matches!(result, Err(ConfigurationError::Admission(SectionAdmissionError::HttpCors(_)))),
        "{result:?}"
    );
}

/// RFC 9110 clause 7.6.3: the alias is one `received-by` token, so one
/// holding a space is refused while loading.
#[test]
#[serial]
fn a_source_alias_that_is_not_a_token_is_refused_at_load() {
    let path = write_temp_config("[identity]\nsource_alias = \"broker a\"\n");
    let result = load_admitted(Some(path.clone()), ListenerOverrides::default()); // clone: the file is removed after loading
    fs::remove_file(path).expect("temp config should be removed");
    assert!(
        matches!(result, Err(ConfigurationError::Admission(SectionAdmissionError::SourceAlias(_)))),
        "{result:?}"
    );
}

#[test]
#[serial]
fn env_disabling_cursor_pagination_with_the_cursor_default_is_refused_at_load() {
    unsafe { env::set_var("ANDROMEDA__EXTENSIONS__CURSOR_PAGINATION", "disabled") };
    let loaded = load_admitted(None, ListenerOverrides::default());
    unsafe { env::remove_var("ANDROMEDA__EXTENSIONS__CURSOR_PAGINATION") };
    assert!(matches!(loaded, Err(ConfigurationError::Admission(SectionAdmissionError::Pagination(_)))));
}
