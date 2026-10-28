//! What the filters a subscription request writes are admitted under.
//!
//! Each term is the broker's for every request, so one value carries all
//! three from composition to validation; a stored subscription is never held
//! to them again, and a lowered limit leaves it readable.

use andromeda_ngsi_ld_query::{
    filter::{complexity::FilterComplexityLimits, language_targeting::LanguageTargeting},
    geo::admission::GeoQueryAdmission,
};

/// The broker's terms for the `geoQ`, `q` and `csf` a subscription request
/// writes.
#[derive(Debug, Clone, Copy)]
pub struct SubscriptionFilterAdmission {
    /// What a written `geoQ` is admitted under.
    pub geo_q: GeoQueryAdmission,
    /// The most a written `q` may ask of the broker before it is too complex
    /// to resolve (clause 5.5.6).
    pub q_complexity: FilterComplexityLimits,
    /// The broker's `q_target_languages` level, which the target languages of
    /// a written `q` and `csf` are admitted under.
    pub language_targeting: LanguageTargeting,
}
