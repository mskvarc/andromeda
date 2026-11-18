//! Why a geo-query (clause 4.10) is refused: a member Table 5.2.13-1 requires
//! is missing or malformed, or its reference geometry passes the broker's
//! limits.

use crate::geo::{reference_budget::ReferenceBudgetError, written_geo_query::RequiredMember};
use andromeda_ngsi_ld_geometry::{geo_rel::GeoRelParseError, geojson::GeometryError};
use andromeda_ngsi_ld_lexicon::reference::NgsiLdReferenceError;
use thiserror::Error;

/// The clause 4.10 rule a geo-query broke, or the broker limit it passed.
#[derive(Debug, Error)]
pub enum GeoQueryError {
    /// A member Table 5.2.13-1 gives cardinality 1 is absent or `null`.
    #[error("a geo-query requires its '{0}' member (Table 5.2.13-1)")]
    MissingMember(RequiredMember),
    /// Some but not all of the four required geo components were provided.
    #[error("geo: all of georel, geometry, coordinates must be provided together")]
    Incomplete,
    /// The relation is not defined by clause 4.10.
    #[error("invalid georel: {0}")]
    InvalidGeorel(#[source] GeoRelParseError),
    /// The geometry name is not one of the `GeoJSON` types supported by NGSI-LD.
    #[error("invalid geo-query geometry type: {0}")]
    InvalidGeometryType(#[source] strum::ParseError),
    /// Coordinates held as text are not valid JSON.
    #[error("geo: coordinates is not valid JSON")]
    InvalidCoordinates(#[source] serde_json::Error),
    /// The `geometry` and `coordinates` pair does not form a `GeoJSON` geometry.
    ///
    /// Carries the geometry diagnostic verbatim so that a position with an
    /// ordinate count RFC 7946 section 3.1.1 gives no meaning to reaches the
    /// client as a `BadRequestData` detail naming that count, rather than as a
    /// bare "invalid geometry".
    #[error("geo-query reference geometry: {0}")]
    InvalidReferenceGeometry(#[source] GeometryError),
    /// `georel=near` carried neither `maxDistance` nor `minDistance`.
    #[error("geo: near georel requires maxDistance or minDistance")]
    NearWithoutDistance,
    /// `georel=near` carried both `maxDistance` and `minDistance` while the
    /// `near_distance_band` extension is off (clause 4.10 admits one).
    #[error("geo: near georel takes one of maxDistance or minDistance; both together need the near_distance_band extension")]
    NearDistanceBand,
    /// The expanded `geoproperty` is not usable as an attribute name (clause 5.2.4).
    #[error("invalid attribute name: {0}")]
    InvalidGeoproperty(#[source] NgsiLdReferenceError),
    /// The reference geometry holds more positions than the broker resolves
    /// in one query.
    #[error(transparent)]
    ReferenceBudget(#[from] ReferenceBudgetError),
}
