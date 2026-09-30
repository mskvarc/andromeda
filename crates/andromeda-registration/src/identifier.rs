//! Registration identifiers: the one a client names and the one the broker
//! assigns when it names none.

use crate::{error::RegistrationError, registration_invariant::RegistrationInvariant, registration_validation_reason::RegistrationValidationReason};
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, uri::NGSI_LD_URN_NID};
use urn_rs::UrnBuilder;
use uuid::Uuid;

/// Read the identifier a client named its registration by (clause 5.2.9).
///
/// # Errors
///
/// Returns [`RegistrationValidationReason::IdNotUri`] carrying why `raw` is not
/// an identifier, including one past the identifier bound.
pub(crate) fn client_registration_id(raw: &str) -> Result<Identifier, RegistrationError> {
    raw.parse()
        .map_err(|source| RegistrationError::Validation(RegistrationValidationReason::IdNotUri { source }))
}

/// Mint a broker-assigned registration identifier (clause 5.9.2.4).
pub(crate) fn generate_registration_id() -> Result<Identifier, RegistrationError> {
    let urn = UrnBuilder::new(NGSI_LD_URN_NID, &format!("ContextSourceRegistration:{}", Uuid::new_v4())).build()?;
    Identifier::try_from(urn).map_err(|error| RegistrationInvariant::MintedIdentifier(error).into())
}

#[cfg(test)]
mod tests {
    use crate::{error::RegistrationError, identifier::client_registration_id, registration_validation_reason::RegistrationValidationReason};
    use andromeda_ngsi_ld_lexicon::{identifier::IdentifierParseError, identifier_length::IDENTIFIER_BYTE_LIMIT};

    #[test]
    fn a_registration_id_past_the_limit_is_refused_with_its_reason() {
        let prefix = "urn:ngsi-ld:ContextSourceRegistration:";
        let raw = format!("{prefix}{}", "a".repeat(IDENTIFIER_BYTE_LIMIT + 1 - prefix.len()));
        let error = client_registration_id(&raw).expect_err("an over-long id");
        assert!(
            matches!(
                error,
                RegistrationError::Validation(RegistrationValidationReason::IdNotUri {
                    source: IdentifierParseError::TooLong(_)
                })
            ),
            "{error:?}"
        );
        assert!(error.to_string().contains("1025 bytes"), "{error}");
    }

    #[test]
    fn a_registration_id_within_the_limit_is_read_verbatim() {
        let id = client_registration_id("urn:ngsi-ld:ContextSourceRegistration:1").expect("a valid id");
        pretty_assertions::assert_eq!(id.as_str(), "urn:ngsi-ld:ContextSourceRegistration:1");
    }
}
