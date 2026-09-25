//! The number `csr_delivery_state.status` stores a registration's health as.
//!
//! Written as exhaustive matches in both directions, so a status a later
//! revision of clause 5.2.9 adds fails to compile here, where its persisted
//! number is decided.

use andromeda_ngsi_ld_model::registration::status::CsrStatus;
use andromeda_postgresql_codes::csr_status::CsrStatusCode;

/// The registry member the store persists for a health status.
#[must_use]
pub const fn stored_status(status: CsrStatus) -> CsrStatusCode {
    match status {
        CsrStatus::Ok => CsrStatusCode::Ok,
        CsrStatus::Failed => CsrStatusCode::Failed,
    }
}

/// The health status a stored registry member names.
#[must_use]
pub const fn status_of(code: CsrStatusCode) -> CsrStatus {
    match code {
        CsrStatusCode::Ok => CsrStatus::Ok,
        CsrStatusCode::Failed => CsrStatus::Failed,
    }
}

#[cfg(test)]
mod tests {
    use crate::csource_registration::status_code::{status_of, stored_status};
    use andromeda_ngsi_ld_model::registration::status::CsrStatus;
    use andromeda_postgresql_codes::csr_status::CsrStatusCode;
    use pretty_assertions::assert_eq;
    use strum::IntoEnumIterator;

    #[test]
    fn every_health_status_survives_a_round_trip_through_its_stored_number() {
        for status in CsrStatus::iter() {
            assert_eq!(status_of(stored_status(status)), status);
        }
    }

    /// The registry's label is the clause 5.2.9 token the domain enum renders.
    #[test]
    fn the_stored_number_of_a_status_is_the_one_its_token_is_registered_under() {
        for code in CsrStatusCode::ALL {
            assert_eq!(code.label(), status_of(*code).to_string());
        }
    }
}
