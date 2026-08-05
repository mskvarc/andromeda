//! The PostgreSQL spelling of the NGSI-LD query languages.
//!
//! `andromeda-ngsi-ld-query` compiles a request into the languages ETSI GS CIM
//! 009 v1.9.1 defines; this crate says how each of them is written in
//! PostgreSQL. A `q` filter becomes a probe against the `attribute_current`
//! projection, a `georel` becomes a PostGIS predicate, a `scopeQ` becomes an
//! `lquery` match, a type selection becomes a predicate over the `types` array
//! — plus the primitives those renderings are built from: typed bind values,
//! schema identifiers, jsonpath and literal escaping, and the two operators
//! sea-query has no native constructor for.
//!
//! Everything here is a fragment. Assembling fragments into a statement is the
//! job of the crate that owns the statement:
//! `andromeda-postgresql-context-query` for current entity state and context
//! source registrations, `andromeda-postgresql-temporal-query` for an entity's
//! temporal evolution. Both port groups render the same fragments, which is why
//! the fragments are here and not in either of them.
//!
//! `sqlx` stays out: a rendered statement leaves as a [`built::BuiltQuery`],
//! and the store binds its [`bind::BindValue`]s.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod attribute_name_id;
pub mod bind;
pub mod built;
pub mod error;
pub mod geo_predicate;
pub mod jsonb;
pub mod literal;
pub mod ltree;
pub mod operator;
pub mod postgis;
pub mod q_predicate;
pub mod schema;
pub mod scope_predicate;
pub mod type_predicate;

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
        const FORBIDDEN: &[&str] = &["sqlx", "axum", "reqwest", "confique", "clap", "tokio"];
        /// In-workspace crates this one may name. Anything else is an upward or
        /// sideways edge.
        const ALLOWED_IN_WORKSPACE: &[&str] = &[
            "andromeda-ngsi-ld-geometry",
            "andromeda-ngsi-ld-lexicon",
            "andromeda-ngsi-ld-query",
            "andromeda-ngsi-ld-representation",
            "andromeda-ngsi-ld-vocabulary",
            "andromeda-postgresql-encoding",
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
