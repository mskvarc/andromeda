//! Why an ordering (clause 4.23) is refused: a malformed `orderBy` term, a
//! malformed or missing reference geometry, or a term a temporal query cannot
//! order by.

use crate::{
    filter::{display::serialize_attr_path, filter_error::FilterError, value_path::ValuePath},
    geo::reference_budget::ReferenceBudgetError,
    ordering::{icu_collation::IcuCollationError, ordering_direction::OrderingDirection},
};
use andromeda_ngsi_ld_geometry::geojson::GeometryError;
use thiserror::Error;

/// The clause 4.23 rule an ordering broke, or the broker limit its reference
/// geometry passed.
#[derive(Debug, Error)]
pub enum OrderingError {
    /// The term did not satisfy the clause 4.9 `ValuePath` production that
    /// clause 4.23.3 delegates its `AttrName` to.
    #[error("invalid orderBy member: {0}")]
    Path(#[source] FilterError),
    /// The term used an unsupported direction.
    #[error("invalid orderBy direction: {0}")]
    InvalidDirection(#[source] strum::ParseError),
    /// The term carried the `[*]` language-suppression marker. Clause 4.9 gives `[*]` to a `q` predicate; clause 4.23.3 has no such marker, so an ordering that suppressed language selection would order by a value the clause never says how to choose.
    #[error("invalid orderBy member: the [*] language marker belongs to the q filter grammar, not to orderBy")]
    LanguageSuppression,
    /// The term named an RFC 4647 language range. A range selects every key it matches, and clause 4.23.3 gives no rule for which of several selected strings the ordering reads.
    #[error("invalid orderBy member: a language range selects several languages, and orderBy orders by one value")]
    LanguageRange,
    /// A distance ordering was requested with no geometry to measure from.
    #[error("orderBy: a distance ordering requires orderFrom")]
    DistanceWithoutReference,
    /// A temporal query's `orderBy` named an Entity member other than `id`.
    ///
    /// Clause 5.7.4.4: "If the ordering parameter is present and refers an
    /// entity name other than "id", then an error of type `BadRequestData`
    /// shall be raised."
    #[error("a temporal query can only be ordered by the Entity member id, not by '{}'", serialize_attr_path(.0))]
    TemporalMember(Box<ValuePath>),
    /// A temporal query's `orderBy` asked for a distance ordering.
    ///
    /// Table 6.18.3.2-1 admits only the sorting styles `asc` and `desc`, and
    /// clause 4.23.1 limits sorting by distance to `GeoProperties`, which the
    /// `id` member clause 5.7.4.3 confines the ordering to is not.
    #[error("a temporal query can only be ordered 'asc' or 'desc', not '{0}'")]
    TemporalDirection(OrderingDirection),
    /// The `collation` identifier was invalid.
    #[error(transparent)]
    Collation(#[from] IcuCollationError),
    /// The geometry type and coordinates do not form a valid RFC 7946 geometry. The diagnostic is carried verbatim, so an `orderFrom` position with an unusable ordinate count reaches the client naming that count.
    #[error("orderFrom reference geometry: {0}")]
    InvalidReferenceGeometry(#[source] GeometryError),
    /// The coordinates do not re-encode as the `GeoJSON` they were read from.
    #[error("{0}")]
    InvalidCoordinates(#[source] serde_json::Error),
    /// `orderGeometry` was sent without the `orderFrom` coordinates it types.
    #[error("orderGeometry requires orderFrom")]
    GeometryWithoutCoordinates,
    /// The reference geometry of a distance ordering holds more positions
    /// than the broker resolves in one query.
    #[error(transparent)]
    ReferenceBudget(#[from] ReferenceBudgetError),
}
