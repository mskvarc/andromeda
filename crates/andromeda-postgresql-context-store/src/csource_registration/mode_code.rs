//! The number the registration's `mode` column stores a forwarding mode as.
//!
//! Written as exhaustive matches in both directions, so a mode a later revision
//! of clause 4.3.6 adds fails to compile here, where its persisted number is
//! decided.

use andromeda_ngsi_ld_model::registration::forwarding_mode::ForwardingMode;
use andromeda_postgresql_codes::csr_mode::CsrModeCode;

/// The registry member the store persists for a forwarding mode.
#[must_use]
pub const fn stored_mode(mode: ForwardingMode) -> CsrModeCode {
    match mode {
        ForwardingMode::Inclusive => CsrModeCode::Inclusive,
        ForwardingMode::Exclusive => CsrModeCode::Exclusive,
        ForwardingMode::Redirect => CsrModeCode::Redirect,
        ForwardingMode::Auxiliary => CsrModeCode::Auxiliary,
    }
}

/// The forwarding mode a stored registry member names.
#[must_use]
pub const fn mode_of(code: CsrModeCode) -> ForwardingMode {
    match code {
        CsrModeCode::Inclusive => ForwardingMode::Inclusive,
        CsrModeCode::Exclusive => ForwardingMode::Exclusive,
        CsrModeCode::Redirect => ForwardingMode::Redirect,
        CsrModeCode::Auxiliary => ForwardingMode::Auxiliary,
    }
}

#[cfg(test)]
mod tests {
    use crate::csource_registration::mode_code::{mode_of, stored_mode};
    use andromeda_ngsi_ld_model::registration::forwarding_mode::ForwardingMode;
    use andromeda_postgresql_codes::csr_mode::CsrModeCode;
    use pretty_assertions::assert_eq;
    use strum::IntoEnumIterator;

    #[test]
    fn every_forwarding_mode_survives_a_round_trip_through_its_stored_number() {
        for mode in ForwardingMode::iter() {
            assert_eq!(mode_of(stored_mode(mode)), mode);
        }
    }

    /// The registry's label is the clause 4.3.6 token the domain enum renders.
    #[test]
    fn the_stored_number_of_a_mode_is_the_one_its_token_is_registered_under() {
        for code in CsrModeCode::ALL {
            assert_eq!(code.label(), mode_of(*code).to_string());
        }
    }
}
