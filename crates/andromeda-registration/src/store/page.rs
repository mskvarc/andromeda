//! One backend-neutral page of registrations (ETSI GS CIM 009 clause 5.10.2).

use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;

/// One backend-neutral page of registrations and its full match count.
#[derive(Debug)]
pub struct RegistrationPage {
    /// Registrations in the requested deterministic page.
    pub registrations: Vec<ContextSourceRegistration>,
    /// Total registrations matching the request before pagination.
    pub total: u64,
}
