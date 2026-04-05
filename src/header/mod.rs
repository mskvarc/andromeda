//! Typed HTTP headers for NGSI-LD protocol (RFC 7240, ETSI GS CIM 009 clause 6.3.6).

mod accept;
mod ngsild_language;
mod prefer;
mod preference_applied;
mod util;

pub use self::{
    accept::{Accept, ResponseFormat},
    ngsild_language::{LanguageRange, NgsiLdLanguage},
    prefer::{GeoJsonBodyPreference, NgsiLdVersion, Prefer},
    preference_applied::PreferenceApplied,
    util::quality::Quality,
};
