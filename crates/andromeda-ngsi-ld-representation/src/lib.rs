//! How an NGSI-LD resource is rendered, as opposed to what it is.
//!
//! ETSI GS CIM 009 v1.9.1 separates a resource from its representation: clause
//! 4.5.1 names the normalized, `keyValues`, simplified and concise renderings,
//! clause 4.5.8 says when the system-generated attributes of clause 4.8 appear
//! in one, and clause 4.5.21 says whether a linked entity is nested or flat.
//! None of those decisions changes what an entity *is*, which is why they are
//! not members of the data model, and every one of them is read on both sides
//! of it — by the query languages that compile a request and by the renderers
//! that answer one.
//!
//! It sits below both: a representation option is expressible in terms of the
//! lexical layer alone, so nothing here names a resource, a query or a store.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod join_mode;
pub mod response_format;
pub mod sys_attrs;

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
        const ALLOWED_IN_WORKSPACE: &[&str] = &["andromeda-ngsi-ld-lexicon", "andromeda-ngsi-ld-vocabulary"];

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
