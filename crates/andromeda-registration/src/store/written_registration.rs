//! A registration as one committed write left it.

use andromeda_ngsi_ld_model::{csource_change_revision::CsourceChangeRevision, registration::ContextSourceRegistration};

/// The registration a create or update wrote, read in the same transaction,
/// and the revision the write drew.
///
/// A change is evaluated against this state rather than one read later, when
/// a subsequent write may already have replaced it.
#[derive(Clone, Debug)]
pub struct WrittenRegistration {
    /// The revision the write drew.
    pub revision: CsourceChangeRevision,
    /// The registration exactly as the write left it.
    pub registration: ContextSourceRegistration,
}
