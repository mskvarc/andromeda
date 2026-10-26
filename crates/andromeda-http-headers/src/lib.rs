//! The typed NGSI-LD HTTP header set, and the values those headers carry.
//!
//! ETSI GS CIM 009 v1.9.1 clause 6.3 defines the headers an NGSI-LD interaction
//! uses: the `NGSILD-*` family it introduces, and the general HTTP headers --
//! `Accept`, `Prefer`, `Preference-Applied`, `Link`, `Via` -- whose NGSI-LD
//! reading is narrower than the RFC's. Every one of them is a
//! [`headers::Header`] implementation over a validated type, so a handler never
//! sees a header value as text.
//!
//! The value types live here with them rather than in the representation crate,
//! because a header's grammar and the value it spells are one thing: a
//! `Quality` exists because RFC 9110 puts `;q=` in a header, a
//! [`ResponseMediaType`](media_type::ResponseMediaType) is what `Accept`
//! negotiates and `Content-Type` reports, and a
//! [`LanguageFilter`](language::LanguageFilter) is the `Accept-Language`
//! grammar that clause 4.15 also permits as a query parameter. Splitting the
//! token from its value would put one clause in two crates.
//!
//! Nothing here is an axum type. Every `headers::Header` impl is on a local
//! type over `http`'s `HeaderName` and `HeaderValue`, so the same headers serve
//! the server, the notification transports and the forwarding client without
//! any of them depending on a web framework.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod accept;
pub mod alt_svc;
pub mod api_root_scheme;
pub mod forwarded_protocol;
pub mod geo_json_body;
pub mod json_ld_accept;
pub mod language;
pub mod link;
pub mod listener_scheme;
pub mod location;
pub mod media_type;
pub mod ngsild_content_range;
pub mod ngsild_entity_map;
pub mod ngsild_results_count;
pub mod ngsild_snapshot;
pub mod ngsild_tenant;
pub mod ngsild_warning;
pub mod prefer;
pub mod preference;
pub mod preference_applied;
pub mod quality;
pub mod received_protocol;
pub mod via;
pub mod warning;
pub mod x_request_id;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-configuration",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-representation",
        "andromeda-ngsi-ld-vocabulary",
    ],
    allow_workspace_dev_dependencies: [
    ],
    allow_production_dependencies: [],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
