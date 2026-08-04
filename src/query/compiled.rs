//! Compiled (context-expanded, validated) query representations.
//!
//! These types are the output of the query compilation step and serve as
//! the input to the SQL generation layer. All attribute names and type IRIs
//! are fully expanded, pagination defaults are resolved, and the `q` expression
//! has been parsed into an AST.

use crate::{
    query::{error::QueryError, q::ast::Expr},
    renderer::SysAttrsInclusion,
};
use andromeda_domain::{
    geo_rel::GeoRel,
    geometry::Geometry,
    temporal::{AggrMethod, TemporalFormat, TimeProp, TimeRel},
};
use chrono::{DateTime, Utc};
use langtag::LangTagBuf;
use thiserror::Error;

/// Pagination parameters shared by every compiled query variant.
///
/// Consolidates the `limit`, `offset`, and `count` trio that used to be
/// repeated on `CompiledEntityQuery`, `CompiledTemporalQuery`, and
/// `CompiledCsrQuery`. Construct via `Pagination { ... }` or `Pagination::DEFAULT`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pagination {
    /// Maximum number of results to return (NGSI-LD 5.5.6).
    pub limit: u32,
    /// Number of results to skip for pagination (NGSI-LD 5.5.6).
    pub offset: u32,
    /// Whether to emit a total-count header (NGSI-LD 6.3.13).
    pub count: bool,
}

impl Pagination {
    /// Default pagination: limit 20, offset 0, count disabled.
    pub const DEFAULT: Pagination = Pagination {
        limit: 20,
        offset: 0,
        count: false,
    };
}

impl Default for Pagination {
    fn default() -> Pagination {
        Pagination::DEFAULT
    }
}

/// Boolean expression over entity type IRIs (NGSI-LD S4.17).
///
/// Supports AND (`And`), OR (`Or`), and single type (`Type`) nodes.
/// Constructed by parsing the `type` query parameter's selection language
/// grammar and expanding compact type names to full IRIs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeExpr {
    /// Matches entities whose types include this specific IRI.
    Type(String),
    /// Matches entities whose types satisfy all children (semicolon-separated).
    And(Vec<TypeExpr>),
    /// Matches entities whose types satisfy any child (comma/pipe-separated).
    Or(Vec<TypeExpr>),
}

/// Purge mode for the DELETE entities-by-query operation (S5.6.21).
///
/// When `drop` or `keep` parameters are provided, the operation modifies
/// entity attributes instead of deleting entire entities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PurgeMode {
    /// Delete entire matching entities (default behaviour).
    DeleteEntities,
    /// Remove the listed attributes from matching entities.
    Drop(Vec<String>),
    /// Retain only the listed attributes on matching entities, removing all others.
    Keep(Vec<String>),
}

/// NGSI-LD scope query expression string.
///
/// Wraps the raw scope query (`scopeQ`) parameter value, which uses
/// comma for OR and semicolon for AND (e.g. `"/A;/B,/C"`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeQuery(String);

impl ScopeQuery {
    /// Creates a new `ScopeQuery` from a raw scope query string.
    pub const fn new(s: String) -> ScopeQuery {
        ScopeQuery(s)
    }

    /// Returns the inner string as a `&str`.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// ICU collation identifier for `orderBy` string sorting (NGSI-LD S4.23).
///
/// Wraps the raw collation identifier string (e.g. `"und-u-ks-identic"`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IcuCollation(String);

impl IcuCollation {
    /// Creates a new `IcuCollation` from a raw collation identifier string.
    pub const fn new(s: String) -> IcuCollation {
        IcuCollation(s)
    }

    /// Parses and validates an ICU collation identifier.
    pub fn parse(raw: &str) -> Result<IcuCollation, IcuCollationError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(IcuCollationError::Empty);
        }

        let tag = LangTagBuf::new(trimmed.to_string()).map_err(|source| IcuCollationError::InvalidTag {
            value: trimmed.to_string(),
            source,
        })?;

        if !tag.as_str().contains("-u-") {
            return Err(IcuCollationError::MissingUnicodeExtension(trimmed.to_string()));
        }

        Ok(IcuCollation::new(trimmed.to_string()))
    }

    /// Returns the inner string as a `&str`.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Error)]
pub enum IcuCollationError {
    #[error("collation identifier must not be empty")]
    Empty,
    #[error("collation identifier must contain Unicode extension subtags: {0}")]
    MissingUnicodeExtension(String),
    #[error("invalid BCP47 collation identifier '{value}'")]
    InvalidTag {
        value: String,
        #[source]
        source: langtag::InvalidLangTag<String>,
    },
}

/// NGSI-LD entity ID pattern (validated regex).
///
/// Wraps a regex pattern string that has been validated at parse time.
/// The inner `regex::Regex` is pre-compiled for efficient matching.
#[derive(Debug, Clone)]
pub struct IdPattern(regex::Regex);

impl IdPattern {
    /// Creates a new `IdPattern`, validating the regex at construction time.
    pub fn new(pattern: &str) -> Result<IdPattern, QueryError> {
        let re = regex::Regex::new(pattern)?;
        Ok(IdPattern(re))
    }

    /// Returns the original pattern string.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl PartialEq for IdPattern {
    fn eq(&self, other: &IdPattern) -> bool {
        self.0.as_str() == other.0.as_str()
    }
}

impl Eq for IdPattern {}

/// A fully compiled entity query, ready for SQL generation.
///
/// Produced by expanding and validating `EntityQueryParameters` against the
/// active JSON-LD context. All identifiers are expanded IRIs.
#[derive(Debug, Clone)]
pub struct CompiledEntityQuery {
    /// Optional set of entity IDs to filter by (expanded URIs/URNs).
    pub ids: Option<Vec<String>>,
    /// Optional regex pattern for matching entity IDs.
    pub id_pattern: Option<IdPattern>,
    /// Boolean expression over entity type IRIs (S4.17).
    /// `None` when no type filter is specified.
    pub type_expr: Option<TypeExpr>,
    /// Expanded attribute IRIs to filter by. Empty means no attribute filter.
    pub attrs: Vec<String>,
    /// Parsed `q` (query language) expression for property-level filtering.
    pub q_expr: Option<Expr>,
    /// Geo-spatial query filter, if specified.
    pub geo: Option<GeoQuery>,
    /// Reference geometry for distance-based ordering (orderFrom/orderGeometry, S5.2.43).
    pub ordering_geometry: Option<OrderingGeometry>,
    /// Result window: limit, offset, and total-count toggle.
    pub pagination: Pagination,
    /// Expanded attribute IRIs to include in the response (per NGSI-LD 6.3.15).
    pub pick: Vec<String>,
    /// Expanded attribute IRIs to exclude from the response (per NGSI-LD 6.3.15).
    pub omit: Vec<String>,
    /// Ordering terms, each specifying an attribute and direction.
    pub order_by: Vec<CompiledOrderTerm>,
    /// Scope query for filtering entities by their scope hierarchy.
    pub scope_q: Option<ScopeQuery>,
}

/// A single term in an `orderBy` clause after context expansion.
#[derive(Debug, Clone)]
pub struct CompiledOrderTerm {
    /// The fully expanded IRI of the attribute to sort by.
    pub attr_iri: String,
    /// The sort direction (ascending, descending, or distance-based).
    pub direction: OrderDirection,
    /// ICU collation identifier for string sorting (S4.23).
    pub collation: Option<IcuCollation>,
}

/// A reference geometry for distance-based ordering (NGSI-LD S4.23, S5.2.43).
///
/// Constructed from the `orderFrom` and `orderGeometry` query parameters.
/// When present, distance-based `orderBy` terms use this geometry as the
/// reference point instead of the geo-query geometry.
#[derive(Debug, Clone, PartialEq)]
pub struct OrderingGeometry {
    /// The reference geometry the distance predicates measure from.
    pub geometry: Geometry,
}

impl OrderingGeometry {
    /// Wrap a geometry into an ordering reference.
    pub const fn new(geometry: Geometry) -> OrderingGeometry {
        OrderingGeometry { geometry }
    }
}

/// A compiled geo-spatial query, per NGSI-LD 4.10.
///
/// All four components (georel, geometry, coordinates, geoproperty) are required
/// when a geo query is present.
#[derive(Debug, Clone, PartialEq)]
pub struct GeoQuery {
    /// The spatial relationship to evaluate.
    pub georel: GeoRel,
    /// The GeoJSON geometry type (e.g. "Point", "Polygon").
    pub geometry: String,
    /// The GeoJSON coordinates as a parsed JSON value.
    pub coordinates: serde_json::Value,
    /// The fully expanded IRI of the GeoProperty to query against.
    pub geoproperty_iri: String,
}

/// A fully compiled Context Source Registration (CSR) query.
///
/// Used for querying `context_source_registrations` per NGSI-LD 5.10.2.
/// CSR queries support a subset of entity query features plus temporal filtering
/// against the registration's observation/management intervals.
#[derive(Debug, Clone)]
pub struct CompiledCsrQuery {
    /// Optional set of CSR registration IDs to filter by.
    pub ids: Option<Vec<String>>,
    /// Optional expanded entity type IRI to match against registered entity types.
    pub entity_type: Option<String>,
    /// Optional ID pattern to match against registered entity ID patterns.
    pub id_pattern: Option<IdPattern>,
    /// Optional expanded attribute IRIs to match against registered property/relationship names.
    pub attrs: Option<Vec<String>>,
    /// Parsed `q` expression for filtering against CSR `additional_properties`.
    pub q_expr: Option<Expr>,
    /// Geo-spatial query filter against the CSR `location`.
    pub geo: Option<GeoQuery>,
    /// Temporal filter against the CSR observation or management interval.
    pub temporal: Option<CompiledCsrTemporalFilter>,
    /// Result window: limit, offset (count ignored for CSR queries).
    pub pagination: Pagination,
}

/// Temporal filter component of a CSR query.
///
/// Filters Context Source Registrations based on their temporal coverage,
/// matching against the `observationInterval` or `managementInterval` fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledCsrTemporalFilter {
    /// Temporal relationship operator.
    pub timerel: TimeRel,
    /// The reference time point.
    pub time_at: DateTime<Utc>,
    /// The end time point for `between` queries.
    pub end_time_at: Option<DateTime<Utc>>,
    /// The time property to filter on.
    /// Determines whether the observation or management interval is checked.
    pub timeproperty: Option<TimeProp>,
}

/// A single parsed ordering term from the `orderBy` parameter.
///
/// Contains the compact (pre-expansion) attribute name and the requested
/// sort direction. Will be expanded to a `CompiledOrderTerm` during compilation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderTerm {
    /// The compact attribute name to sort by.
    pub attr: String,
    /// The sort direction.
    pub direction: OrderDirection,
}

/// Sort direction for `orderBy` terms per NGSI-LD S4.23.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderDirection {
    /// Sort in ascending order.
    Asc,
    /// Sort in descending order.
    Desc,
    /// Sort by distance in ascending order from the geo query reference geometry (S4.23).
    DistAsc,
    /// Sort by distance in descending order from the geo query reference geometry (S4.23).
    DistDesc,
}

impl OrderDirection {
    /// Whether this is a distance-based ordering direction.
    pub const fn is_distance(&self) -> bool {
        matches!(self, OrderDirection::DistAsc | OrderDirection::DistDesc)
    }
}

/// Indicates the origin of the `lastN` limit in a temporal query.
///
/// This enum replaces two boolean fields (`system_last_n` and
/// `user_specified_last_n`) to clearly distinguish between the four possible
/// states of lastN handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LastNOrigin {
    /// The user explicitly specified a `lastN` parameter within the system cap.
    /// No truncation detection needed -- the user gets exactly what they asked for.
    UserSpecified,
    /// The user specified `lastN` but it exceeded the system cap, so the system
    /// reduced it. Triggers truncation detection (206 Partial Content) but uses
    /// user-specified direction for Content-Range.
    UserSpecifiedCapped,
    /// No user `lastN` was specified; the system applied DEFAULT_LAST_N as a cap.
    /// Triggers truncation detection (206 Partial Content) with system direction
    /// for Content-Range.
    SystemCapped,
    /// No `lastN` limit is active (e.g. aggregated queries without lastN).
    None,
}

impl LastNOrigin {
    /// Returns true when the system cap is active and truncation may occur.
    /// This is the case for both `SystemCapped` and `UserSpecifiedCapped`.
    pub const fn is_system_capped(self) -> bool {
        matches!(self, LastNOrigin::SystemCapped | LastNOrigin::UserSpecifiedCapped)
    }

    /// Returns true when the user explicitly provided a `lastN` parameter,
    /// regardless of whether it was further capped by the system.
    pub const fn is_user_specified(self) -> bool {
        matches!(self, LastNOrigin::UserSpecified | LastNOrigin::UserSpecifiedCapped)
    }
}

/// Compiled temporal query ready for SQL building.
///
/// Produced by expanding and validating `TemporalQueryParameters` against the
/// active JSON-LD context. Contains both entity-level filters and temporal-specific
/// parameters needed to query the `temporal_entity_attributes` and `attribute_history` tables.
#[derive(Debug, Clone)]
pub struct CompiledTemporalQuery {
    /// Optional set of entity IDs to filter by (expanded URIs/URNs).
    pub ids: Option<Vec<String>>,
    /// Optional regex pattern for matching entity IDs.
    pub id_pattern: Option<IdPattern>,
    /// Optional set of entity type IRIs to filter by.
    pub types: Option<Vec<String>>,
    /// Parsed `q` expression for property-level filtering.
    pub q_expr: Option<Expr>,
    /// Geo-spatial query filter, if specified.
    pub geo: Option<GeoQuery>,

    /// Temporal relationship operator (`before`, `after`, `between`).
    pub timerel: Option<TimeRel>,
    /// Reference time point for the temporal filter.
    pub time_at: Option<DateTime<Utc>>,
    /// End time point for `between` queries.
    pub end_time_at: Option<DateTime<Utc>>,
    /// The time property to filter on (`observedAt`, `createdAt`, `modifiedAt`, `deletedAt`).
    pub timeproperty: TimeProp,

    /// Expanded attribute IRIs to filter temporal data by.
    pub attr_filter: Vec<String>,
    /// Dataset IDs to filter temporal data by. `"@none"` matches the default (null) dataset.
    pub dataset_id_filter: Vec<String>,

    /// Maximum number of temporal instances per attribute to return.
    pub last_n: Option<u32>,
    /// The origin of the `lastN` limit, distinguishing user-specified values
    /// from system-imposed caps (which trigger 206 Partial Content).
    pub last_n_origin: LastNOrigin,
    /// The temporal response format (normalized, temporalValues, or aggregatedValues).
    pub format: TemporalFormat,
    /// Aggregation methods to apply when `format` is `aggregatedValues`.
    pub aggr_methods: Vec<AggrMethod>,
    /// ISO 8601 duration for aggregation bucket size (e.g. `"PT1H"`).
    pub aggr_period_duration: Option<String>,

    /// Result window: limit, offset, and total-count toggle.
    pub pagination: Pagination,

    /// Whether to include system-generated attributes (`createdAt`, `modifiedAt`).
    pub sys_attrs: SysAttrsInclusion,

    /// Scope query for filtering entities by their scope hierarchy.
    ///
    /// For entity queries, filters by current scopes via `entities.scope_paths`.
    /// For temporal queries, filters by historical scopes via `scope_history`,
    /// bounded by the temporal time window (NGSI-LD 4.18).
    pub scope_q: Option<ScopeQuery>,
}
