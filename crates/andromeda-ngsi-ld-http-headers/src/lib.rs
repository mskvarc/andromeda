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
//! grammar that clause 6.3.5 also permits as a query parameter. Splitting the
//! token from its value would put one clause in two crates.
//!
//! Nothing here is an axum type. Every `headers::Header` impl is on a local
//! type over `http`'s `HeaderName` and `HeaderValue`, so the same headers serve
//! the server, the notification transports and the forwarding client without
//! any of them depending on a web framework.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod accept;
pub mod geo_json_body;
pub mod language;
pub mod link;
pub mod media_type;
pub mod ngsild_dataset_id;
pub mod ngsild_entity_map;
pub mod ngsild_join;
pub mod ngsild_join_level;
pub mod ngsild_language;
pub mod ngsild_response_format;
pub mod ngsild_results_count;
pub mod ngsild_scope;
pub mod ngsild_snapshot;
pub mod ngsild_tenant;
pub mod ngsild_warning;
pub mod prefer;
pub mod preference;
pub mod preference_applied;
pub mod quality;
pub mod version;
pub mod via;

#[cfg(test)]
mod boundary {
    /// The layer rule this crate is on the inside of.
    ///
    /// Asserted here rather than by a central test so the failure names the
    /// crate that broke the rule, and so a crate added to the workspace cannot
    /// be forgotten by a table it was never added to. The rule itself is read
    /// once, in `andromeda-layer-guard`, which every crate carries as a
    /// dev-dependency and which declares nothing of its own.
    /// A header is expressible in terms of the lexical and representation
    /// layers alone, so nothing above them -- a resource, a query, a store, a
    /// web framework -- may be named here.
    #[test]
    fn this_crate_declares_only_what_its_layer_permits() {
        /// Third-party crates this layer must never ship.
        const FORBIDDEN: &[&str] = &["sqlx", "sea-query", "geozero", "bytes", "axum", "reqwest", "confique", "clap", "tokio"];
        /// In-workspace crates this one may name, in any dependency table.
        /// Anything else is an upward or sideways edge.
        const ALLOWED_IN_WORKSPACE: &[&str] = &[
            "andromeda-layer-guard",
            "andromeda-ngsi-ld-lexicon",
            "andromeda-ngsi-ld-representation",
            "andromeda-ngsi-ld-vocabulary",
        ];

        andromeda_layer_guard::manifest::assert_layer(env!("CARGO_MANIFEST_DIR"), FORBIDDEN, ALLOWED_IN_WORKSPACE);
    }
}

#[cfg(test)]
mod dialect {
    /// No storage dialect may be written here, in any form.
    ///
    /// The manifest guard above reads dependency names, which is exactly what a
    /// dialect emitted as a string constant slips past: a specification crate
    /// can spell an entire SQL predicate without declaring `sea-query`.
    #[test]
    fn no_source_file_in_this_crate_emits_a_storage_dialect() {
        andromeda_layer_guard::dialect::assert_no_dialect(env!("CARGO_MANIFEST_DIR"));
    }
}
