//! In-memory R-tree spatial indexes for geo-query acceleration.
//!
//! Two independent indexes share a common [`SpatialKey`]:
//!
//! - [`SpatialEntityIndex`] accelerates `georel`/`geometry`/`coordinates`
//!   filtering on entity queries by pre-filtering candidate IDs before SQL.
//! - [`SpatialSubscriptionIndex`] accelerates subscription-side geo matching
//!   by pre-filtering candidate subscriptions before exact predicate evaluation.
//!
//! Both trees hold the full geometry per entry and run the same exact
//! `geo::Contains`/`Intersects` predicates as [`crate::service::subscription_matching`],
//! so semantics are identical to the non-accelerated path.

pub mod distance;
pub mod entity_index;
pub mod subscription_index;
pub mod warmup;

use crate::{
    query::compiled::{GeoQuery, TypeExpr},
    tenancy::types::TenantRef,
};
use iref::IriBuf;

/// Partition key for both spatial indexes.
///
/// Trees are partitioned by `(tenant, entity_type_iri, geoproperty_iri)` so
/// each R-tree stays small and selective. The tenant component isolates
/// cross-tenant data; the type and geoproperty components mirror the
/// `SubscriptionCache::by_type` selectivity.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct SpatialKey {
    /// Owning tenant.
    pub tenant: TenantRef,
    /// Expanded entity type IRI (e.g. `https://uri.fiware.org/ns/data-models#Vehicle`).
    pub type_iri: IriBuf,
    /// Expanded geoproperty IRI (e.g. `https://uri.etsi.org/ngsi-ld/location`).
    pub geoproperty_iri: IriBuf,
}

impl SpatialKey {
    /// Construct a new spatial key.
    pub const fn new(tenant: TenantRef, type_iri: IriBuf, geoproperty_iri: IriBuf) -> SpatialKey {
        SpatialKey {
            tenant,
            type_iri,
            geoproperty_iri,
        }
    }
}

/// Collect the union of concrete type IRIs reachable from a [`TypeExpr`].
///
/// Returns `None` when the expression uses `And` nodes the spatial index
/// cannot satisfy (intersection over multi-type entities is not modelled in
/// the tree partitioning). The caller falls back to SQL in that case.
pub fn flatten_or_type_iris(expr: &TypeExpr) -> Option<Vec<IriBuf>> {
    match expr {
        TypeExpr::Type(s) => IriBuf::new(s.clone()).ok().map(|iri| vec![iri]),
        TypeExpr::Or(children) => {
            let mut out = Vec::with_capacity(children.len());
            for child in children {
                out.extend(flatten_or_type_iris(child)?);
            }
            Some(out)
        }
        TypeExpr::And(_) => None,
    }
}

/// Parse a compiled [`GeoQuery`]'s reference geometry into a usable
/// [`geo::Geometry`].
///
/// Returns `None` if the GeoJSON round-trip fails (malformed coordinates,
/// unsupported geometry type). The caller treats this as a signal to fall
/// through to SQL, matching existing behaviour when PostGIS rejects the
/// geometry at query time.
pub fn build_ref_geometry(geo_q: &GeoQuery) -> Option<geo::Geometry<f64>> {
    let geojson_value = serde_json::json!({
        "type": geo_q.geometry,
        "coordinates": geo_q.coordinates,
    });
    let parsed: geojson::GeoJson = geojson_value.to_string().parse().ok()?;
    match parsed {
        geojson::GeoJson::Geometry(g) => geo::Geometry::try_from(g).ok(),
        geojson::GeoJson::Feature(_) | geojson::GeoJson::FeatureCollection(_) => None,
    }
}
