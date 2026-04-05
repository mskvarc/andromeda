//! Typed HTTP headers for NGSI-LD protocol (RFC 7240, ETSI GS CIM 009 clause 6.3.6).

mod prefer;
mod preference_applied;

pub use self::{
    prefer::{GeoJsonBodyPreference, NgsiLdVersion, Prefer},
    preference_applied::PreferenceApplied,
};
