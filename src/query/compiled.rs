//! Compiled (context-expanded, validated) query representations.
//!
//! These types are the output of the query compilation step and serve as
//! the input to the SQL generation layer. All attribute names and type IRIs
//! are fully expanded, pagination defaults are resolved, and the `q` expression
//! has been parsed into an AST.

use crate::{
    domain::temporal::{AggrMethod, TemporalFormat, TimeProp, TimeRel},
    query::{error::QueryError, q::ast::Expr},
    renderer::SysAttrsInclusion,
};
use chrono::{DateTime, Utc};

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

    /// Returns the inner string as a `&str`.
    pub fn as_str(&self) -> &str {
        &self.0
    }
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
        let re = regex::Regex::new(pattern).map_err(|e| QueryError::BadRequestData(format!("Invalid idPattern regex: {}", e)))?;
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
    /// Optional set of entity type IRIs to filter by.
    pub types: Option<Vec<String>>,
    /// Expanded attribute IRIs to filter by. Empty means no attribute filter.
    pub attrs: Vec<String>,
    /// Parsed `q` (query language) expression for property-level filtering.
    pub q_expr: Option<Expr>,
    /// Geo-spatial query filter, if specified.
    pub geo: Option<GeoQuery>,
    /// Reference geometry for distance-based ordering (orderFrom/orderGeometry, S5.2.43).
    pub ordering_geometry: Option<OrderingGeometry>,
    /// Maximum number of results to return.
    pub limit: u32,
    /// Number of results to skip for pagination.
    pub offset: u32,
    /// Whether to include a total count of matching entities in the response.
    pub count: bool,
    /// Expanded attribute IRIs to include in the response (per NGSI-LD 6.3.15).
    pub pick: Vec<String>,
    /// Expanded attribute IRIs to exclude from the response (per NGSI-LD 6.3.15).
    pub omit: Vec<String>,
    /// Ordering terms, each specifying an attribute and direction.
    pub order_by: Vec<CompiledOrderTerm>,
    /// Scope query for filtering entities by their scope hierarchy.
    pub scope_q: Option<ScopeQuery>,
    /// When true, entities must match all specified types (AND semantics).
    /// When false, entities matching any specified type are returned (OR semantics).
    pub type_match_all: bool,
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
    /// The parsed GeoJSON geometry combining type and coordinates.
    pub geometry: geojson::Geometry,
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

/// Geo-spatial relationship operators per NGSI-LD 4.10.
///
/// These correspond to the `georel` query parameter values defined in the spec.
#[derive(Debug, Clone, PartialEq)]
pub enum GeoRel {
    /// Matches entities whose geometry is entirely within the reference geometry.
    Within,
    /// Matches entities whose geometry contains the reference geometry.
    Contains,
    /// Matches entities whose geometry intersects the reference geometry.
    Intersects,
    /// Matches entities whose geometry is disjoint from the reference geometry.
    Disjoint,
    /// Matches entities whose geometry is topologically equal to the reference geometry.
    Equals,
    /// Matches entities whose geometry overlaps the reference geometry.
    Overlaps,
    /// Matches entities within a distance range from the reference geometry.
    /// At least one of `max_distance` or `min_distance` must be specified.
    Near {
        /// Maximum distance in meters. Entities farther than this are excluded.
        max_distance: Option<f64>,
        /// Minimum distance in meters. Entities closer than this are excluded.
        min_distance: Option<f64>,
    },
}

impl GeoRel {
    /// Parses a `georel` query parameter string into a `GeoRel` variant.
    ///
    /// The input may include modifier parameters separated by semicolons,
    /// e.g. `"near;maxDistance==1000"` or `"near;minDistance==50;maxDistance==500"`.
    pub fn parse(s: &str) -> Result<Self, QueryError> {
        let parts: Vec<&str> = s.split(';').collect();
        let rel = parts[0].trim();
        match rel {
            "within" => Ok(GeoRel::Within),
            "contains" => Ok(GeoRel::Contains),
            "intersects" => Ok(GeoRel::Intersects),
            "disjoint" => Ok(GeoRel::Disjoint),
            "equals" => Ok(GeoRel::Equals),
            "overlaps" => Ok(GeoRel::Overlaps),
            "near" => {
                let mut max_distance = None;
                let mut min_distance = None;
                for part in &parts[1..] {
                    let part = part.trim();
                    if let Some(val) = part.strip_prefix("maxDistance==") {
                        max_distance = Some(
                            val.parse::<f64>()
                                .map_err(|_| QueryError::BadRequestData("Invalid maxDistance value".to_string()))?,
                        );
                    } else if let Some(val) = part.strip_prefix("minDistance==") {
                        min_distance = Some(
                            val.parse::<f64>()
                                .map_err(|_| QueryError::BadRequestData("Invalid minDistance value".to_string()))?,
                        );
                    }
                }
                Ok(GeoRel::Near { max_distance, min_distance })
            }
            _ => Err(QueryError::BadRequestData(format!("Unknown geo relationship: {}", rel))),
        }
    }
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
    /// Maximum number of results to return.
    pub limit: u32,
    /// Number of results to skip for pagination.
    pub offset: u32,
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

    /// SQL direction keyword.
    pub const fn sql_dir(&self) -> &'static str {
        match self {
            OrderDirection::Asc | OrderDirection::DistAsc => "ASC",
            OrderDirection::Desc | OrderDirection::DistDesc => "DESC",
        }
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

    /// Maximum number of entities to return.
    pub limit: u32,
    /// Number of entities to skip for pagination.
    pub offset: u32,
    /// Whether to include a total count of matching entities in the response.
    pub count: bool,

    /// Whether to include system-generated attributes (`createdAt`, `modifiedAt`).
    pub sys_attrs: SysAttrsInclusion,
}
