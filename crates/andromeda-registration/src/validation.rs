//! The clause 5.2.9 gate shared by registration creation and update.
//!
//! Member shapes are enforced by the types the registration
//! payload is read into; what remains here are the rules that
//! span members or need the current time: `expiresAt` in the future (clauses
//! 5.9.2.4 and 5.9.3.4), the clause 4.3.6.6 restrictions on
//! `contextSourceInfo`, and the mode rules that need nothing beyond the
//! registration itself.

use crate::{error::RegistrationError, mode_rules::validate_structure, registration_validation_reason::RegistrationValidationReason};
use andromeda_ngsi_ld_model::{
    context_source_info::ContextSourceInfo,
    key_value_pair::KeyValuePair,
    registration::{forwarding_mode::ForwardingMode, info::RegistrationInfo, operation::CsrOperation},
};
use chrono::{DateTime, Utc};

/// The members of a registration — as created, or as resolved after an update
/// merge — that the cross-member rules read.
pub(super) struct RegistrationShape<'a> {
    /// The forwarding mode.
    pub(super) mode: ForwardingMode,
    /// The expanded `information` blocks.
    pub(super) information: &'a [RegistrationInfo],
    /// The effective operations.
    pub(super) operations: &'a [CsrOperation],
    /// The `contextSourceInfo` pairs.
    pub(super) context_source_info: Option<&'a [KeyValuePair]>,
    /// The expiry instant.
    pub(super) expires_at: Option<DateTime<Utc>>,
}

/// Apply the clause 5.2.9 cross-member rules and the clause 4.3.6.6
/// `contextSourceInfo` restrictions, returning the reserved keys read.
pub(super) fn validate_shape<'a>(shape: &RegistrationShape<'a>) -> Result<ContextSourceInfo<'a>, RegistrationError> {
    if shape.expires_at.is_some_and(|expires_at| expires_at <= Utc::now()) {
        return Err(RegistrationError::Validation(RegistrationValidationReason::ExpiresAtInPast));
    }
    let info = match shape.context_source_info {
        Some(pairs) => {
            ContextSourceInfo::try_from(pairs).map_err(|error| RegistrationError::Validation(RegistrationValidationReason::ContextSourceInfo(error)))?
        }
        None => ContextSourceInfo::default(),
    };
    validate_structure(shape.mode, shape.information, shape.operations)?;
    Ok(info)
}

#[cfg(test)]
mod tests {
    use crate::validation::{RegistrationShape, validate_shape};
    use andromeda_ngsi_ld_model::{
        key_value_pair::KeyValuePair,
        registration::{forwarding_mode::ForwardingMode, info::RegistrationInfo, operation::CsrOperation},
    };
    use chrono::{Duration, Utc};
    use serde_json::json;

    fn information() -> Vec<RegistrationInfo> {
        vec![serde_json::from_value(json!({"entities": [{"type": "https://example.org/Vehicle"}]})).expect("valid information")]
    }

    fn pair(key: &str, value: &str) -> KeyValuePair {
        KeyValuePair {
            key: key.into(),
            value: value.into(),
        }
    }

    #[test]
    fn an_expiry_in_the_past_is_rejected_and_one_in_the_future_accepted() {
        let information = information();
        let past = RegistrationShape {
            mode: ForwardingMode::Inclusive,
            information: &information,
            operations: &[CsrOperation::FederationOps],
            context_source_info: None,
            expires_at: Some(Utc::now() - Duration::hours(1)),
        };
        assert!(validate_shape(&past).is_err());
        let future = RegistrationShape {
            expires_at: Some(Utc::now() + Duration::hours(1)),
            ..past
        };
        assert!(validate_shape(&future).is_ok());
    }

    #[test]
    fn a_context_source_info_outside_clause_4_3_6_6_is_rejected() {
        let information = information();
        for pairs in [
            vec![pair("accept", "text/plain")],
            vec![pair("contentType", "application/xml")],
            vec![pair("ngsildConformance", "v1")],
            vec![pair("jsonldContext", "http://exa mple.org/ctx")],
        ] {
            let shape = RegistrationShape {
                mode: ForwardingMode::Inclusive,
                information: &information,
                operations: &[CsrOperation::FederationOps],
                context_source_info: Some(&pairs),
                expires_at: None,
            };
            assert!(validate_shape(&shape).is_err(), "{pairs:?} must be rejected");
        }
        let valid = [pair("accept", "application/ld+json"), pair("Authorization", "Bearer x")];
        let shape = RegistrationShape {
            mode: ForwardingMode::Inclusive,
            information: &information,
            operations: &[CsrOperation::FederationOps],
            context_source_info: Some(&valid),
            expires_at: None,
        };
        assert!(validate_shape(&shape).is_ok());
    }

    #[test]
    fn the_mode_structure_rules_are_part_of_the_gate() {
        let information = information();
        let shape = RegistrationShape {
            mode: ForwardingMode::Auxiliary,
            information: &information,
            operations: &[CsrOperation::CreateEntity],
            context_source_info: None,
            expires_at: None,
        };
        assert!(validate_shape(&shape).is_err());
    }
}
