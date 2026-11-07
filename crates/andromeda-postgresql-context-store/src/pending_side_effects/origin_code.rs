//! The numbers a `pending_change_event` stores a change's origin as.
//!
//! The domain's origins and the migration-owned registry that numbers them are
//! variant-for-variant identical, and the translation is written out as
//! exhaustive matches in both directions, so an origin a later revision adds
//! fails to compile here, where its stored number is decided.

use andromeda_ngsi_ld_model::notification::ChangeOrigin;
use andromeda_postgresql_codes::change_origin::ChangeOriginCode;

/// The registry member the schema persists for an origin.
#[must_use]
pub(crate) const fn stored_origin(origin: ChangeOrigin) -> ChangeOriginCode {
    match origin {
        ChangeOrigin::Local => ChangeOriginCode::Local,
        ChangeOrigin::Forwarded => ChangeOriginCode::Forwarded,
    }
}

/// The origin a stored registry member names.
#[must_use]
pub(crate) const fn origin_of(code: ChangeOriginCode) -> ChangeOrigin {
    match code {
        ChangeOriginCode::Local => ChangeOrigin::Local,
        ChangeOriginCode::Forwarded => ChangeOrigin::Forwarded,
    }
}

#[cfg(test)]
mod tests {
    use crate::pending_side_effects::origin_code::{origin_of, stored_origin};
    use andromeda_ngsi_ld_model::notification::ChangeOrigin;
    use andromeda_postgresql_codes::change_origin::ChangeOriginCode;

    #[test]
    fn every_change_origin_survives_a_round_trip_through_its_stored_number() {
        for origin in [ChangeOrigin::Local, ChangeOrigin::Forwarded] {
            pretty_assertions::assert_eq!(origin_of(stored_origin(origin)), origin);
        }
        pretty_assertions::assert_eq!(ChangeOriginCode::ALL.len(), 2);
    }
}
