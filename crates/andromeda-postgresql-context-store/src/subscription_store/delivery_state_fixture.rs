//! The subscription, branch and instants the statement tests of a
//! subscription's delivery state address.

use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use chrono::{DateTime, TimeZone, Utc};

/// The subscription every statement addresses.
pub(super) fn subscription() -> Identifier {
    "urn:ngsi-ld:Subscription:one".parse().expect("valid")
}

/// The branch every statement addresses.
pub(super) fn target() -> ContextTarget {
    ContextTarget::Live
}

/// `seconds` past noon on 17 August 2026.
pub(super) fn instant(seconds: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 8, 17, 12, 0, seconds).single().expect("valid instant")
}
