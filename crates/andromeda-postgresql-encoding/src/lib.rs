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
    /// Asserted here rather than by a central test so the failure names the
    /// crate that broke the rule, and so a crate added to the workspace cannot
    /// be forgotten by a table it was never added to. The rule itself is read
    /// once, in `andromeda-layer-guard`, which every crate carries as a
    /// dev-dependency and which declares nothing of its own.
    /// `geozero` and `bytes` are this crate's whole reason to exist, so only the
    /// driver, the transports and the runtime are forbidden. It converts between
    /// a geometry and a byte buffer; if it ever needs a `Name`, it has stopped
    /// being a codec.
    #[test]
    fn this_crate_declares_only_what_its_layer_permits() {
        /// In-workspace crates this one may name, in any dependency table.
        /// Anything else is an upward or sideways edge.
        const ALLOWED_IN_WORKSPACE: &[&str] = &["andromeda-layer-guard", "andromeda-ngsi-ld-geometry"];

        // The encoding layer reads and writes EWKB, so it ships the geometry
        // codec `geozero` and the `bytes` buffer it works over.
        let forbidden = andromeda_layer_guard::manifest::baseline_forbidden_except(&["bytes", "geozero"]);
        andromeda_layer_guard::manifest::assert_layer(env!("CARGO_MANIFEST_DIR"), &forbidden, ALLOWED_IN_WORKSPACE);
    }
}
