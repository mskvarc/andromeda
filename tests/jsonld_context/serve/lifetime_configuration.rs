//! Refusing a context lifetime the broker cannot state (study E11).
//!
//! Both `[jsonld]` lifetimes are advertised or applied in whole seconds (the
//! Serve `max-age`, the in-process revalidation span), so a zero or sub-second
//! span is refused when the configuration is read, rather than truncated to
//! zero or silently replaced by a default. The broker binary under test is run
//! with `config validate`, which reads and checks the configuration exactly as
//! startup does.

use std::process::{Command, Output};

/// Run `andromeda config validate` with one environment override.
fn validate_with(variable: &str, value: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_andromeda"))
        .args(["config", "validate"])
        .env(variable, value)
        .output()
        .expect("the broker binary runs")
}

#[test]
fn a_zero_or_sub_second_context_lifetime_is_refused_at_startup() {
    for (variable, setting) in [
        ("ANDROMEDA__JSONLD__HOSTED_CONTEXT_REVALIDATION", "hosted_context_revalidation"),
        ("ANDROMEDA__JSONLD__CACHE_TTL", "cache_ttl"),
    ] {
        for value in ["500ms", "0s"] {
            let validated = validate_with(variable, value);
            let report = format!("{}{}", String::from_utf8_lossy(&validated.stdout), String::from_utf8_lossy(&validated.stderr));

            assert!(!validated.status.success(), "{variable}={value} was accepted: {report}");
            assert!(report.to_ascii_lowercase().contains(setting), "the refusal names {setting}: {report}");
        }
    }
}

#[test]
fn a_whole_second_context_lifetime_is_accepted() {
    for variable in ["ANDROMEDA__JSONLD__HOSTED_CONTEXT_REVALIDATION", "ANDROMEDA__JSONLD__CACHE_TTL"] {
        for value in ["1s", "2s", "5m"] {
            let validated = validate_with(variable, value);
            let report = format!("{}{}", String::from_utf8_lossy(&validated.stdout), String::from_utf8_lossy(&validated.stderr));

            assert!(validated.status.success(), "{variable}={value} was refused: {report}");
        }
    }
}
