//! Broker-assigned registration identifiers.

use crate::error::RegistrationError;
use andromeda_ngsi_ld_lexicon::uri::{NGSI_LD_URN_NID, Uri};
use urn_rs::UrnBuilder;
use uuid::Uuid;

/// Mint a broker-assigned registration identifier (clause 5.10.2).
pub(super) fn generate_registration_id() -> Result<Uri, RegistrationError> {
    let urn = UrnBuilder::new(NGSI_LD_URN_NID, &format!("ContextSourceRegistration:{}", Uuid::new_v4())).build()?;
    Ok(urn.into())
}
