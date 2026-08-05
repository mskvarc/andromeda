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
mod tests {
    /// The boundary the three extractions bought. If a forbidden dependency
    /// reappears in one of these manifests, a storage, transport or runtime
    /// concern has drifted below the line it was moved above, and this test is
    /// the only thing that notices.
    #[test]
    fn each_crate_below_the_store_declares_only_what_its_layer_permits() {
        /// A crate's manifest and the dependencies it must not declare.
        struct Boundary {
            /// Crate name, for the assertion message.
            name: &'static str,
            /// The manifest text, embedded at compile time.
            manifest: &'static str,
            /// Dependency names forbidden at this crate's layer.
            forbidden: &'static [&'static str],
        }

        /// Storage drivers, transports and the runtime: forbidden everywhere below the store.
        const INFRASTRUCTURE: &[&str] = &["sqlx", "axum", "reqwest", "confique", "clap", "tokio"];
        /// The PostgreSQL wire-format codecs, forbidden in every specification crate.
        const BACKEND_CODECS: &[&str] = &["sqlx", "geozero", "bytes", "axum", "reqwest", "confique", "clap", "tokio"];

        let boundaries = [
            Boundary {
                name: "andromeda-domain",
                manifest: include_str!("../../andromeda-domain/Cargo.toml"),
                forbidden: BACKEND_CODECS,
            },
            Boundary {
                name: "andromeda-ngsi-ld-geometry",
                manifest: include_str!("../../andromeda-ngsi-ld-geometry/Cargo.toml"),
                forbidden: BACKEND_CODECS,
            },
            Boundary {
                name: "andromeda-ngsi-ld-lexicon",
                manifest: include_str!("../../andromeda-ngsi-ld-lexicon/Cargo.toml"),
                forbidden: BACKEND_CODECS,
            },
            Boundary {
                name: "andromeda-ngsi-ld-query",
                manifest: include_str!("../../andromeda-ngsi-ld-query/Cargo.toml"),
                // A query language that names sea-query is a query language with
                // a backend in it; clause 4.9 has no dialect.
                forbidden: &["sqlx", "sea-query", "geozero", "bytes", "axum", "reqwest", "confique", "clap", "tokio"],
            },
            Boundary {
                // `geozero` and `bytes` are this crate's whole reason to exist,
                // so only the driver, transport and runtime are forbidden.
                name: "andromeda-postgresql-encoding",
                manifest: include_str!("../Cargo.toml"),
                forbidden: INFRASTRUCTURE,
            },
            Boundary {
                name: "andromeda-postgresql-query",
                manifest: include_str!("../../andromeda-postgresql-query/Cargo.toml"),
                forbidden: INFRASTRUCTURE,
            },
        ];

        for boundary in boundaries {
            let dependencies: Vec<&str> = boundary
                .manifest
                .split("[dependencies]")
                .nth(1)
                .expect("manifest has a [dependencies] table")
                .split("\n[")
                .next()
                .expect("dependency table is terminated")
                .lines()
                .collect();
            for name in boundary.forbidden {
                assert!(
                    !dependencies.iter().any(|line| line.trim_start().starts_with(name)),
                    "{} must not depend on {name}",
                    boundary.name
                );
            }
        }
    }
}
