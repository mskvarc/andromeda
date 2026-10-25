//! Registration identifiers assigned by the broker when the client names none.

use crate::{error::RegistrationError, registration_invariant::RegistrationInvariant};
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, ngsi_ld_urn::ContextSourceRegistrationId};

/// Mint a broker-assigned registration identifier (clause 5.9.2.4).
pub(crate) fn generate_registration_id() -> Result<Identifier, RegistrationError> {
    ContextSourceRegistrationId::mint()
        .map(Identifier::from)
        .map_err(|error| RegistrationInvariant::MintedIdentifier(error).into())
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::identifier::generate_registration_id;

    #[test]
    fn a_minted_registration_id_is_a_context_source_registration_urn() {
        let id = generate_registration_id().expect("an identifier is minted");
        assert!(id.as_str().starts_with("urn:ngsi-ld:ContextSourceRegistration:"), "{id}");
    }
}
