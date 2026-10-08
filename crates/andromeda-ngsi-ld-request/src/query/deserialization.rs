//! Deserializers for structured query-string values.

use andromeda_ngsi_ld_geometry::query_coordinates::QueryCoordinates;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier, reference::NgsiLdReference};
use andromeda_ngsi_ld_query::{
    dataset::DatasetSelector,
    filter::{expression::Expr, parser},
    geo::input::{GeoQueryInput, GeoQueryInputError},
    pattern::Pattern,
    projection::grammar::{ProjectionList, RawProjectionTerm},
};
use iso8601_duration::Duration as Iso8601Duration;
use serde::{Deserialize, Deserializer, de::Error as DeserializeError};
use std::num::{IntErrorKind, NonZeroU32};

/// A complete optional geo-query decoded from URL parameters.
#[derive(Debug, Clone, Default)]
pub struct OptionalGeoQuery(pub Option<GeoQueryInput>);

#[derive(Deserialize)]
/// Wire members used to assemble one complete geo-query.
struct GeoQueryParameters {
    /// Spatial relationship token.
    georel: Option<String>,
    /// `GeoJSON` geometry type.
    geometry: Option<String>,
    /// `GeoJSON` coordinates.
    coordinates: Option<QueryCoordinates>,
    /// Optional `GeoProperty` reference.
    geoproperty: Option<NgsiLdReference>,
}

/// The rejection of a `georel` sent without `geometry` and `coordinates`:
/// parameters incompatible with the operation (clause 6.3.20).
pub const GEOREL_WITHOUT_GEOMETRY: &str = "georel requires geometry and coordinates";

/// The rejection of `geometry` and/or `coordinates` sent without the relation
/// that would make them a geoquery (clause 4.10), which clauses 5.7.2.4 and
/// 5.10.2.4 class as a geoquery that is not syntactically valid.
pub const INCOMPLETE_GEOQUERY: &str = "geometry and coordinates do not form a geoquery without georel";

impl<'de> Deserialize<'de> for OptionalGeoQuery {
    fn deserialize<D>(deserializer: D) -> Result<OptionalGeoQuery, D::Error>
    where
        D: Deserializer<'de>,
    {
        let parameters = GeoQueryParameters::deserialize(deserializer)?;
        let (georel, geometry_type, coordinates) = match (parameters.georel, parameters.geometry, parameters.coordinates) {
            (None, None, None) => return Ok(OptionalGeoQuery(None)),
            (Some(georel), Some(geometry_type), Some(coordinates)) => (georel, geometry_type, coordinates),
            // A relation with no reference geometry to relate to is a parameter
            // the operation cannot use at all (clause 6.3.20).
            (Some(_), None | Some(_), None) | (Some(_), None, Some(_)) => {
                return Err(DeserializeError::custom(GEOREL_WITHOUT_GEOMETRY));
            }
            // A reference geometry with no relation, or half a reference
            // geometry, is a geoquery that is not valid as per clause 4.10.
            (None, None | Some(_), None | Some(_)) => {
                return Err(DeserializeError::custom(INCOMPLETE_GEOQUERY));
            }
        };
        decode_geo_query(&georel, &geometry_type, coordinates, parameters.geoproperty)
            .map(Some)
            .map(OptionalGeoQuery)
            .map_err(DeserializeError::custom)
    }
}

/// Decode a complete geo-query at a request boundary.
///
/// # Errors
///
/// Returns [`GeoQueryDecodeError::Decode`] when the relationship token, the
/// geometry type or the coordinates do not form a clause 4.10 geoquery.
pub fn decode_geo_query(
    georel: &str,
    geometry_type: &str,
    coordinates: QueryCoordinates,
    geoproperty: Option<NgsiLdReference>,
) -> Result<GeoQueryInput, GeoQueryDecodeError> {
    GeoQueryInput::from_wire(georel, geometry_type, coordinates, geoproperty).map_err(GeoQueryDecodeError::from)
}

/// Geo-query decoding failure at a request boundary.
#[derive(Debug, thiserror::Error)]
pub enum GeoQueryDecodeError {
    /// The geo-query wire form could not be decoded; the inner error names which
    /// member (`georel`, `coordinates`, `geometry`) was at fault and why.
    #[error(transparent)]
    Decode(#[from] GeoQueryInputError),
}

/// Deserialize an optional comma-separated entity identifier list.
///
/// NGSI-LD clause 4.6.2 admits only URNs and URLs as entity identifiers. Empty
/// members are malformed input, so the request is rejected while extracting the
/// query rather than leaving a later compiler to reinterpret the wire value.
///
/// # Errors
///
/// Returns a deserialization error when a member is empty or is neither a URN
/// nor a URL.
pub fn deserialize_optional_uri_csv<'de, D>(deserializer: D) -> Result<Option<Vec<Identifier>>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = Option::<String>::deserialize(deserializer)?;
    raw.map(|value| {
        value
            .split(',')
            .map(str::trim)
            .map(|member| {
                if member.is_empty() {
                    return Err(DeserializeError::custom("entity identifier list contains an empty member"));
                }
                member.parse::<Identifier>().map_err(DeserializeError::custom)
            })
            .collect()
    })
    .transpose()
}

/// Deserialize an optional non-empty `idPattern` as a compiled regular expression.
///
/// # Errors
///
/// Returns a deserialization error when the value is blank or is not a valid
/// regular expression.
pub fn deserialize_optional_pattern<'de, D>(deserializer: D) -> Result<Option<Pattern>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = Option::<String>::deserialize(deserializer)?;
    raw.map(|value| {
        if value.trim().is_empty() {
            return Err(DeserializeError::custom("Invalid idPattern: empty value"));
        }
        value.parse().map_err(DeserializeError::custom)
    })
    .transpose()
}

/// Deserialize an optional NGSI-LD `q` expression into its syntax tree.
///
/// # Errors
///
/// Returns a deserialization error when the expression does not satisfy the
/// clause 4.9 filter grammar.
pub fn deserialize_optional_filter_expr<'de, D>(deserializer: D) -> Result<Option<Expr>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = Option::<String>::deserialize(deserializer)?;
    raw.map(|value| parser::parse(&value).map_err(DeserializeError::custom)).transpose()
}

/// Deserialize an optional comma-separated list of NGSI-LD attribute names.
///
/// # Errors
///
/// Returns a deserialization error for an empty member or a name that violates
/// the clause 4.6.2 reference grammar.
pub fn deserialize_optional_attribute_names<'de, D>(deserializer: D) -> Result<Option<Vec<AttributeName>>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = Option::<String>::deserialize(deserializer)?;
    raw.map(|value| {
        value
            .split(',')
            .map(str::trim)
            .map(|member| {
                if member.is_empty() {
                    return Err(DeserializeError::custom("attribute name list contains an empty member"));
                }
                member.parse().map_err(DeserializeError::custom)
            })
            .collect()
    })
    .transpose()
}

/// Deserialize an optional `pick` / `omit` raw projection list (clause 4.21,
/// with the projection grammar also defined in clause 4.21).
///
/// The whole wire value is parsed as one [`ProjectionList`] rather than
/// split on `,` here first: a linked-entity term such as
/// `observation{temperature,humidity}` uses `,` inside its `{...}` group too,
/// so splitting before parsing would tear the group apart. Comma (and `|`)
/// splitting is entirely the grammar's own job.
///
/// # Errors
///
/// Returns a deserialization error when the value does not satisfy the
/// clause 4.21 projection-list grammar.
pub fn deserialize_optional_projection_terms<'de, D>(deserializer: D) -> Result<Option<Vec<RawProjectionTerm>>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = Option::<String>::deserialize(deserializer)?;
    raw.map(|value| value.parse::<ProjectionList>().map(Vec::from).map_err(DeserializeError::custom))
        .transpose()
}

/// Deserialize an optional comma-separated list of dataset selectors.
///
/// Clause 4.5.5 defines `datasetId` as a list of URIs. Empty entries and
/// non-URI values are malformed request data at the extraction boundary.
///
/// # Errors
///
/// Returns a deserialization error when a member is empty or is neither a URI
/// nor the `@none` selector.
pub fn deserialize_optional_dataset_selectors<'de, D>(deserializer: D) -> Result<Option<Vec<DatasetSelector>>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = Option::<String>::deserialize(deserializer)?;
    raw.map(|value| {
        value
            .split(',')
            .map(str::trim)
            .map(|member| {
                if member.is_empty() {
                    return Err(DeserializeError::custom("dataset identifier list contains an empty member"));
                }
                member.parse().map_err(DeserializeError::custom)
            })
            .collect()
    })
    .transpose()
}

/// Deserialize one dataset selector for a mutation endpoint.
///
/// Unlike query filtering, mutation `datasetId` is singleton-valued. Keeping
/// the whole wire value in the parser means a comma-separated list cannot be
/// accidentally accepted as one opaque identifier.
///
/// # Errors
///
/// Returns a deserialization error when the value carries a comma, is empty,
/// or is neither a URI nor the `@none` selector.
pub fn deserialize_optional_dataset_selector<'de, D>(deserializer: D) -> Result<Option<DatasetSelector>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = Option::<String>::deserialize(deserializer)?;
    raw.map(|value| {
        let value = value.trim();
        if value.contains(',') {
            return Err(DeserializeError::custom("datasetId must contain one URI or @none selector"));
        }
        if value.is_empty() {
            return Err(DeserializeError::custom("datasetId must not be empty"));
        }
        value.parse().map_err(DeserializeError::custom)
    })
    .transpose()
}

/// The rejection of a `joinLevel` URL parameter that is no integer at all: a
/// request that is syntactically invalid (Table 5.5.2-1 `InvalidRequest`).
pub const JOIN_LEVEL_NOT_AN_INTEGER: &str = "joinLevel is not an integer";

/// Deserialize an optional `joinLevel` URL parameter as a positive integer
/// (clause 4.5.23).
///
/// A value that is no integer (`1.5`, `abc`, the empty value) is refused with
/// [`JOIN_LEVEL_NOT_AN_INTEGER`]. An integer outside the positive range is
/// input that does not meet the requirements of the operation, refused with
/// any other message.
///
/// # Errors
///
/// Returns a deserialization error when the value is not an integer, or is an
/// integer below 1 or beyond `u32`.
pub fn deserialize_optional_join_level<'de, D>(deserializer: D) -> Result<Option<NonZeroU32>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = Option::<String>::deserialize(deserializer)?;
    raw.map(|value| {
        let out_of_range = || DeserializeError::custom(format!("joinLevel {value} is not a positive integer"));
        match value.parse::<i64>() {
            Ok(level) => u32::try_from(level).ok().and_then(NonZeroU32::new).ok_or_else(out_of_range),
            Err(error) if matches!(error.kind(), IntErrorKind::PosOverflow | IntErrorKind::NegOverflow) => Err(out_of_range()),
            Err(_not_an_integer) => Err(DeserializeError::custom(JOIN_LEVEL_NOT_AN_INTEGER)),
        }
    })
    .transpose()
}

/// Deserialize an optional ISO-8601 calendar duration at the request boundary.
///
/// # Errors
///
/// Returns a deserialization error when the value is not an ISO-8601 duration.
pub fn deserialize_optional_iso8601_duration<'de, D>(deserializer: D) -> Result<Option<Iso8601Duration>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = Option::<String>::deserialize(deserializer)?;
    raw.map(|value| value.parse().map_err(|error| DeserializeError::custom(format!("{error:?}"))))
        .transpose()
}

#[cfg(test)]
mod tests {
    use crate::query::deserialization::{
        GEOREL_WITHOUT_GEOMETRY,
        INCOMPLETE_GEOQUERY,
        JOIN_LEVEL_NOT_AN_INTEGER,
        OptionalGeoQuery,
        deserialize_optional_join_level,
    };
    use serde::Deserialize;
    use std::num::NonZeroU32;

    #[derive(Deserialize)]
    struct Carrier {
        #[serde(flatten)]
        geo: OptionalGeoQuery,
    }

    fn rejection(query: &str) -> String {
        serde_urlencoded::from_str::<Carrier>(query).err().expect("rejected").to_string()
    }

    /// Clause 6.3.20: a relation alone is a parameter the operation cannot use.
    #[test]
    fn a_georel_without_its_reference_geometry_is_reported_as_such() {
        assert!(rejection("georel=near%3BmaxDistance%3D%3D1000").contains(GEOREL_WITHOUT_GEOMETRY));
        assert!(rejection("georel=within&geometry=Point").contains(GEOREL_WITHOUT_GEOMETRY));
        assert!(rejection("georel=within&coordinates=%5B1%2C2%5D").contains(GEOREL_WITHOUT_GEOMETRY));
    }

    /// Clause 4.10: a reference geometry without a relation is not a geoquery.
    #[test]
    fn a_reference_geometry_without_a_georel_is_an_incomplete_geoquery() {
        assert!(rejection("geometry=Point&coordinates=%5B1%2C2%5D").contains(INCOMPLETE_GEOQUERY));
        assert!(rejection("geometry=Point").contains(INCOMPLETE_GEOQUERY));
        assert!(rejection("coordinates=%5B1%2C2%5D").contains(INCOMPLETE_GEOQUERY));
    }

    #[derive(Deserialize)]
    struct JoinLevelCarrier {
        #[serde(rename = "joinLevel", default, deserialize_with = "deserialize_optional_join_level")]
        join_level: Option<NonZeroU32>,
    }

    fn join_level(query: &str) -> Result<Option<NonZeroU32>, String> {
        serde_urlencoded::from_str::<JoinLevelCarrier>(query)
            .map(|carrier| carrier.join_level)
            .map_err(|error| error.to_string())
    }

    /// Table 5.5.2-1: a value that is no integer is a syntactically invalid
    /// request, an integer below 1 input that does not meet the requirements
    /// of the operation.
    #[test]
    fn a_non_integer_join_level_is_reported_apart_from_an_integer_below_one() {
        for not_an_integer in ["joinLevel=1.5", "joinLevel=abc", "joinLevel=1e1", "joinLevel="] {
            pretty_assertions::assert_eq!(join_level(not_an_integer), Err(JOIN_LEVEL_NOT_AN_INTEGER.to_owned()), "{not_an_integer}");
        }
        for out_of_range in ["joinLevel=0", "joinLevel=-1", "joinLevel=4294967296", "joinLevel=99999999999999999999"] {
            let error = join_level(out_of_range).expect_err(out_of_range);
            assert!(!error.contains(JOIN_LEVEL_NOT_AN_INTEGER), "{out_of_range}: {error}");
        }
        pretty_assertions::assert_eq!(join_level("joinLevel=3"), Ok(NonZeroU32::new(3)));
        pretty_assertions::assert_eq!(join_level(""), Ok(None));
    }

    #[test]
    fn no_geo_member_is_no_geoquery_and_the_full_triple_decodes() {
        let none: Carrier = serde_urlencoded::from_str("").expect("parses");
        assert!(none.geo.0.is_none());
        let full: Carrier = serde_urlencoded::from_str("georel=within&geometry=Point&coordinates=%5B1%2C2%5D").expect("parses");
        assert!(full.geo.0.is_some());
    }
}
