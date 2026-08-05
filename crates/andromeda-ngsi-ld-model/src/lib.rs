//! The Andromeda NGSI-LD domain model.
//!
//! This crate holds the resources ETSI GS CIM 009 v1.9.1 defines — entities,
//! attributes, subscriptions, registrations and the discovery resources — built
//! on the lexical types of `andromeda-ngsi-ld-lexicon` and the IRI constants of
//! `andromeda-ngsi-ld-vocabulary`. It knows nothing about transport,
//! persistence, or the broker runtime, and every layer above it depends on
//! these types rather than on each other's.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod attribute;
pub mod attribute_list;
pub mod entity;
pub mod entity_map;
pub mod entity_type;
pub mod entity_type_list;
pub mod expanded;
pub mod jsonld_context;
pub mod notification;
pub mod registration;
pub mod snapshot;
pub mod snapshot_filter;
pub mod snapshot_query;
pub mod subscription;
pub mod tenant_list;

#[cfg(test)]
mod boundary {
    /// The layer rule this crate is on the inside of.
    ///
    /// A crate asserts its own manifest rather than a central test asserting
    /// everyone's: the failure then names the crate that broke the rule, and a
    /// crate added to the workspace cannot be forgotten by a table it was never
    /// added to.
    #[test]
    fn this_crate_declares_only_what_its_layer_permits() {
        /// Third-party crates this layer must never declare.
        const FORBIDDEN: &[&str] = &["sqlx", "sea-query", "geozero", "bytes", "axum", "reqwest", "confique", "clap", "tokio"];
        /// In-workspace crates this one may name. Anything else is an upward or
        /// sideways edge.
        const ALLOWED_IN_WORKSPACE: &[&str] = &[
            "andromeda-ngsi-ld-geometry",
            "andromeda-ngsi-ld-lexicon",
            "andromeda-ngsi-ld-query",
            "andromeda-ngsi-ld-representation",
            "andromeda-ngsi-ld-vocabulary",
        ];

        let manifest = include_str!("../Cargo.toml");
        let dependencies: Vec<&str> = manifest
            .split("[dependencies]")
            .nth(1)
            .unwrap_or_else(|| unreachable!("manifest has a [dependencies] table"))
            .split("\n[")
            .next()
            .unwrap_or_else(|| unreachable!("dependency table is terminated"))
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .collect();

        for line in &dependencies {
            let name = line.split(['=', '.', ' ']).next().unwrap_or_default();
            assert!(!FORBIDDEN.contains(&name), "this crate must not depend on {name}");
            if name.starts_with("andromeda-") {
                assert!(ALLOWED_IN_WORKSPACE.contains(&name), "{name} is not an edge this crate's layer permits");
            }
        }
    }
}
