//! Typed HTTP headers for NGSI-LD protocol (RFC 7240, ETSI GS CIM 009 clause 6.3.6).

mod accept;
pub mod link;
mod ngsild_entity_map;
mod ngsild_language;
mod ngsild_results_count;
mod ngsild_snapshot;
mod ngsild_tenant;
mod ngsild_warning;
mod prefer;
mod preference_applied;
mod util;
mod via;

pub use self::{
    accept::{Accept, ResponseFormat},
    link::Link,
    ngsild_entity_map::NgsiLdEntityMap,
    ngsild_language::{LanguageRange, NgsiLdLanguage},
    ngsild_results_count::NgsiLdResultsCount,
    ngsild_snapshot::NgsiLdSnapshot,
    ngsild_tenant::NgsiLdTenant,
    ngsild_warning::{NgsiLdWarning, NgsiLdWarningHeader},
    prefer::{GeoJsonBodyPreference, NgsiLdVersion, Prefer},
    preference_applied::PreferenceApplied,
    util::quality::Quality,
    via::Via,
};
