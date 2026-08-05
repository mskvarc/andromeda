//! Geospatial values and relations for NGSI-LD.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.7 models a GeoProperty value as a GeoJSON
//! geometry (IETF RFC 7946) in WGS84 / CRS84, and clause 4.10 defines the
//! `georel` spatial relationships a query may assert over one. This crate owns
//! both, and nothing else: it is a specification crate, so the one encoding it
//! carries is the one the specification names.
//!
//! PostGIS's EWKB is deliberately absent. It is a backend wire format, and it
//! lives in `andromeda-postgresql-encoding` alongside the other formats that are
//! properties of PostgreSQL rather than of NGSI-LD.
//!
//! It is a leaf: it names no other crate in this workspace, and it deliberately
//! knows nothing about persistence, transport or broker policy.
//!
//! Spatial *indexing* is not here either. An R-tree partitioned by tenant, type
//! and GeoProperty is a broker decision about which entities are worth
//! comparing; what a comparison means is the part that belongs to geometry, and
//! that is [`relation::matches`].

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod distance;
pub mod geo_rel;
pub mod geometry;
pub mod relation;

#[cfg(test)]
mod boundary {
    /// The layer rule this crate is on the inside of.
    ///
    /// A crate asserts its own manifest rather than a central test asserting
    /// everyone's: the failure then names the crate that broke the rule, and a
    /// crate added to the workspace cannot be forgotten by a table it was never
    /// added to.
    ///
    /// This crate is a leaf: it names no other crate in the workspace, because
    /// every geo-aware layer depends on it and an upward edge from here reaches
    /// all of them.
    #[test]
    fn this_crate_declares_only_what_its_layer_permits() {
        /// Third-party crates this layer must never declare.
        const FORBIDDEN: &[&str] = &["sqlx", "sea-query", "geozero", "bytes", "axum", "reqwest", "confique", "clap", "tokio"];
        /// In-workspace crates this one may name. Anything else is an upward or
        /// sideways edge.
        const ALLOWED_IN_WORKSPACE: &[&str] = &[];

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

#[cfg(test)]
mod dialect {
    /// No storage dialect may be written here, in any form.
    ///
    /// The manifest guard above reads dependency names, which is exactly what a
    /// dialect emitted as a string constant slips past: a specification crate
    /// can spell an entire SQL predicate without declaring `sea-query`. Six
    /// items did — a comparison operator, a POSIX regex operator, an `AND`
    /// composer, a JSONB cast, and two column mappings on a resource — and
    /// nothing in the workspace could see them.
    #[test]
    fn no_source_file_in_this_crate_emits_a_storage_dialect() {
        /// Fragments that only ever appear in a rendering, never in a grammar.
        const DIALECT: &[&str] = &[
            "::numeric",
            "::boolean",
            "::timestamptz",
            "::lquery",
            "::geography",
            "ST_",
            "jsonb_",
            "= ANY(",
            "SELECT ",
        ];

        for (path, source) in SOURCES {
            // Comments may name the counterpart a backend renders — that is how
            // two readings of one clause stay comparable. Only code is scanned.
            let code: String = source.lines().filter(|line| !line.trim_start().starts_with("//")).collect();
            for fragment in DIALECT {
                assert!(
                    !code.contains(fragment),
                    "{path} emits '{fragment}'; how a value is spelled in a store belongs to that backend"
                );
            }
        }
    }

    /// Every source file in this crate, so a new one cannot be added outside
    /// the guard without the list saying so.
    const SOURCES: &[(&str, &str)] = &[
        ("distance.rs", include_str!("distance.rs")),
        ("geo_rel.rs", include_str!("geo_rel.rs")),
        ("geometry.rs", include_str!("geometry.rs")),
        ("relation.rs", include_str!("relation.rs")),
    ];
}
