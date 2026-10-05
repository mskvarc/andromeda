//! What one expiry sweep deleted.

use andromeda_ngsi_ld_model::{csource_change_revision::CsourceChangeRevision, registration::ContextSourceRegistration};

/// The registrations one expiry sweep deleted, and the revision the sweep
/// drew: every deletion committed together, so they share it.
#[derive(Debug)]
pub struct ExpiredDeletion {
    /// The revision the sweep drew.
    pub revision: CsourceChangeRevision,
    /// Each deleted registration as it stood before the deletion.
    pub registrations: Vec<ContextSourceRegistration>,
}
