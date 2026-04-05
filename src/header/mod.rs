//! Typed HTTP headers for NGSI-LD protocol (RFC 7240, ETSI GS CIM 009 clause 6.3.6).

mod accept;
pub mod link;
mod ngsild_language;
mod prefer;
mod preference_applied;
mod util;

pub use self::{
    accept::{Accept, ResponseFormat},
    link::{insert_jsonld_contexts, jsonld_context_link, link_from_values, link_header_string, link_header_value, link_value},
    ngsild_language::{LanguageRange, NgsiLdLanguage},
    prefer::{GeoJsonBodyPreference, NgsiLdVersion, Prefer},
    preference_applied::PreferenceApplied,
    util::quality::Quality,
};
