//! PostgreSQL's own wire formats, for the values Andromeda binds and reads back.
//!
//! Everything here is a property of the backend rather than of NGSI-LD. EWKB is
//! PostGIS's extended well-known binary; ETSI GS CIM 009 v1.9.1 never mentions
//! it, and says only that a GeoProperty value is GeoJSON per RFC 7946. Encoding
//! therefore belongs on this side of the boundary and not in
//! `andromeda-ngsi-ld-geometry`, which models the specification's geometry and
//! nothing else.
//!
//! `sqlx` stays out. The impls that would put it here forward to `&[u8]` anyway,
//! so the store binds [`ewkb::Ewkb::as_bytes`] directly and this crate stays
//! usable by the query renderers, which must not see a database driver.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod ewkb;

#[cfg(test)]
mod boundary {
    /// The layer rule this crate is on the inside of.
    ///
    /// A crate asserts its own manifest rather than a central test asserting
    /// everyone's: the failure then names the crate that broke the rule, and a
    /// crate added to the workspace cannot be forgotten by a table it was never
    /// added to.
    ///
    /// `geozero` and `bytes` are this crate's whole reason to exist, so only the
    /// driver, the transports and the runtime are forbidden.
    #[test]
    fn this_crate_declares_only_what_its_layer_permits() {
        /// Third-party crates this layer must never declare.
        const FORBIDDEN: &[&str] = &["sqlx", "axum", "reqwest", "confique", "clap", "tokio"];
        /// In-workspace crates this one may name. Anything else is an upward or
        /// sideways edge.
        const ALLOWED_IN_WORKSPACE: &[&str] = &["andromeda-ngsi-ld-geometry"];

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
