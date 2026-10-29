//! The registrations a forwarded read matched, read from the registration
//! store once and split by the forwarding phase that contacts them.
//!
//! Clause 4.3.6.2 contacts the exclusive, redirect and inclusive registrations
//! first and the auxiliary ones once those have answered. Both phases match the
//! same registrations for the same request, so the request reads them once and
//! each phase takes its own.

use crate::{entity_read::mode_admitted, request::CsrModeFilter};
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;

/// The registrations a request matched, whatever their mode.
pub struct MatchedRegistrations(Vec<ContextSourceRegistration>);

/// The registrations of each forwarding phase of one request.
#[derive(Default)]
pub struct PhasedRegistrations {
    /// The exclusive, redirect and inclusive registrations, contacted first.
    pub primary: Vec<ContextSourceRegistration>,
    /// The auxiliary registrations, contacted once the primary phase has
    /// answered (clause 4.3.6.2).
    pub auxiliary: Vec<ContextSourceRegistration>,
}

impl MatchedRegistrations {
    /// The registrations a request matched.
    pub(crate) const fn new(registrations: Vec<ContextSourceRegistration>) -> MatchedRegistrations {
        MatchedRegistrations(registrations)
    }

    /// Every matched registration, for a request with a single phase.
    #[must_use]
    pub fn into_registrations(self) -> Vec<ContextSourceRegistration> {
        self.0
    }

    /// The matched registrations split by the phase that contacts them.
    #[must_use]
    pub fn into_phases(self) -> PhasedRegistrations {
        let (primary, auxiliary) = self
            .0
            .into_iter()
            .partition(|registration| mode_admitted(registration, CsrModeFilter::ExcludeAuxiliary));
        PhasedRegistrations { primary, auxiliary }
    }
}
