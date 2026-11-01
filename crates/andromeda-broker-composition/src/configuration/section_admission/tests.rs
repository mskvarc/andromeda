//! Table admission: a value a domain refuses fails loading.

use crate::configuration::{
    commands::load_admitted,
    error::ConfigurationError,
    section_admission::{SectionAdmissionError, admit_sections},
};
use andromeda_configuration::{
    broker_config::BrokerConfig,
    load::{ListenerOverrides, LoadError},
    validate::{
        bound::{Bound, Quantity, Relation},
        config_key::{ConfigKey, QueueKey, QueueName},
        violation::ConfigurationViolation,
    },
};
use andromeda_ngsi_ld_request::pagination::policy::PaginationPolicy;
use pretty_assertions::assert_eq;
use serial_test::serial;
use std::{
    env,
    fs,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
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

/// Load `contents` as the configuration file through the admission the
/// broker runs at startup.
fn admitted(contents: &str) -> Result<BrokerConfig, ConfigurationError> {
    let path = write_temp_config(contents);
    let result = load_admitted(Some(path.clone()), ListenerOverrides::default()); // clone: the file is removed after loading
    fs::remove_file(path).expect("temp config should be removed");
    result.map(|(_, config)| config)
}

#[test]
fn the_default_configuration_is_admitted() {
    assert!(admit_sections(&BrokerConfig::default()).is_ok());
}

/// `extensions.cursor_pagination.mode = "disabled"` serves offset paging
/// only, whatever else is set, and needs no second key to agree with.
#[test]
#[serial]
fn disabled_cursor_pagination_loads_and_serves_offset_paging_only() {
    let config = admitted("[extensions.cursor_pagination]\nmode = \"disabled\"\n").expect("a disabled cursor mode loads");
    assert_eq!(PaginationPolicy::from(&config.extensions), PaginationPolicy::OffsetOnly);
}

/// The identifier bound is the lexicon's, applied while loading, so an
/// over-long IRI never reaches the source identity route.
#[test]
#[serial]
fn a_source_id_past_the_identifier_bound_is_refused_at_load() {
    let result = admitted(&format!("[federation.identity]\nsource_id = \"urn:ngsi-ld:Node:{}\"\n", "a".repeat(1024)));
    assert!(
        matches!(result, Err(ConfigurationError::Admission(SectionAdmissionError::SourceIdentifier(_)))),
        "{result:?}"
    );
}

/// The configuration declares the domain's nesting ceilings, so the loader
/// refuses past them before any domain reads the table.
#[test]
#[serial]
fn a_nesting_limit_past_its_ceiling_is_refused_at_load() {
    let result = admitted("[request_limits]\nmax_attribute_nesting = 63\nmax_q_nesting = 65\n");
    let expected = [
        ConfigurationViolation::OutOfRange {
            key: ConfigKey::RequestLimitsMaxAttributeNesting,
            value: Quantity::Count(63),
            relation: Relation::AtMost,
            bound: Bound::Fixed(Quantity::Count(62)),
        },
        ConfigurationViolation::OutOfRange {
            key: ConfigKey::RequestLimitsMaxQNesting,
            value: Quantity::Count(65),
            relation: Relation::AtMost,
            bound: Bound::Fixed(Quantity::Count(64)),
        },
    ];
    assert!(
        matches!(&result, Err(ConfigurationError::Load(LoadError::Invalid(invalid))) if invalid.violations() == expected),
        "{result:?}"
    );
    assert!(admitted("[request_limits]\nmax_attribute_nesting = 62\nmax_q_nesting = 64\n").is_ok());
}

/// The queue table refuses the pair before any domain reads it.
#[test]
#[serial]
fn a_side_effect_retry_ceiling_below_its_first_step_is_refused_at_load() {
    let result = admitted("[entity.side_effects.queue]\nretry_interval = \"10s\"\nmax_retry_interval = \"5s\"\n");
    let retry = |queue_key| ConfigKey::Queue(QueueName::EntitySideEffects, queue_key);
    let expected = [ConfigurationViolation::OutOfRange {
        key: retry(QueueKey::MaxRetryInterval),
        value: Quantity::Duration(Duration::from_secs(5)),
        relation: Relation::AtLeast,
        bound: Bound::setting(retry(QueueKey::RetryInterval), Duration::from_secs(10)),
    }];
    assert!(
        matches!(&result, Err(ConfigurationError::Load(LoadError::Invalid(invalid))) if invalid.violations() == expected),
        "{result:?}"
    );
}

#[test]
#[serial]
fn a_delivery_concurrency_past_the_delivery_maximum_is_refused_at_load() {
    let result = admitted("[notifications.delivery]\nmax_concurrent = 4096\n");
    let expected = [ConfigurationViolation::OutOfRange {
        key: ConfigKey::NotificationsDeliveryMaxConcurrent,
        value: Quantity::Count(4096),
        relation: Relation::AtMost,
        bound: Bound::Fixed(Quantity::Count(1024)),
    }];
    assert!(
        matches!(&result, Err(ConfigurationError::Load(LoadError::Invalid(invalid))) if invalid.violations() == expected),
        "{result:?}"
    );
}

#[test]
#[serial]
fn a_zero_federation_cooldown_window_is_refused_at_load() {
    let result = admitted("[federation.cooldown]\nmode = \"constant\"\nretry_interval = \"0s\"\n");
    let expected = [ConfigurationViolation::Zero {
        key: ConfigKey::FederationCooldownRetryInterval,
    }];
    assert!(
        matches!(&result, Err(ConfigurationError::Load(LoadError::Invalid(invalid))) if invalid.violations() == expected),
        "{result:?}"
    );
}

#[test]
#[serial]
fn source_extras_holding_a_date_time_are_refused_at_load() {
    let result = admitted("[federation.identity.extras]\nsince = 1979-05-27\n");
    assert!(
        matches!(result, Err(ConfigurationError::Admission(SectionAdmissionError::SourceExtras(_)))),
        "{result:?}"
    );
}

#[test]
#[serial]
fn a_cors_method_that_is_not_a_method_token_is_refused_at_load() {
    let result = admitted("[http.cors]\nmode = \"enabled\"\nallow_methods = [\"G ET\"]\n");
    assert!(
        matches!(result, Err(ConfigurationError::Admission(SectionAdmissionError::HttpCors(_)))),
        "{result:?}"
    );
}

/// RFC 9110 section 5.5: a field value holds no line break, so a policy
/// holding one is refused while loading.
#[test]
#[serial]
fn a_content_security_policy_with_a_line_break_is_refused_at_load() {
    let result = admitted("[http.security_headers.content_security_policy]\npolicy = \"default-src 'self'\\nscript-src 'none'\"\n");
    assert!(
        matches!(result, Err(ConfigurationError::Admission(SectionAdmissionError::HttpSecurityHeaders(_)))),
        "{result:?}"
    );
}

/// RFC 9110 clause 7.6.3: the alias is one `received-by` token, so one
/// holding a space is refused while loading.
#[test]
#[serial]
fn a_source_alias_that_is_not_a_token_is_refused_at_load() {
    let result = admitted("[federation.identity]\nsource_alias = \"broker a\"\n");
    assert!(
        matches!(result, Err(ConfigurationError::Admission(SectionAdmissionError::SourceAlias(_)))),
        "{result:?}"
    );
}
