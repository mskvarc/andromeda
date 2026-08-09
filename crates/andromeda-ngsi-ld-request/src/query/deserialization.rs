//! Deserializers for structured query-string values.

use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, reference::NgsiLdReference, uri::Uri};
use andromeda_ngsi_ld_query::{
    dataset::DatasetSelector,
    filter::{expression::Expr, parser},
    geo::input::GeoQueryInput,
    pattern::Pattern,
};
use iso8601_duration::Duration as Iso8601Duration;
use serde::{Deserialize, Deserializer};

/// A complete optional geo-query decoded from URL parameters.
#[derive(Debug, Clone, Default)]
pub struct OptionalGeoQuery(pub Option<GeoQueryInput>);

#[derive(Deserialize)]
/// Wire members used to assemble one complete geo-query.
struct GeoQueryParameters {
    /// Spatial relationship token.
    georel: Option<String>,
    /// GeoJSON geometry type.
    geometry: Option<String>,
    /// GeoJSON coordinates.
    coordinates: Option<serde_json::Value>,
    /// Optional GeoProperty reference.
    geoproperty: Option<NgsiLdReference>,
}

impl<'de> Deserialize<'de> for OptionalGeoQuery {
    fn deserialize<D>(deserializer: D) -> Result<OptionalGeoQuery, D::Error>
    where
        D: Deserializer<'de>,
    {
        let parameters = GeoQueryParameters::deserialize(deserializer)?;
        let (georel, geometry_type, coordinates) = match (parameters.georel, parameters.geometry, parameters.coordinates) {
            (None, None, None) => return Ok(OptionalGeoQuery(None)),
            (Some(georel), Some(geometry_type), Some(coordinates)) => (georel, geometry_type, coordinates),
            (None | Some(_), None | Some(_), None | Some(_)) => {
                return Err(serde::de::Error::custom("georel, geometry, and coordinates must be supplied together"));
            }
        };
        decode_geo_query(georel, geometry_type, coordinates, parameters.geoproperty)
            .map(Some)
            .map(OptionalGeoQuery)
            .map_err(serde::de::Error::custom)
    }
}

/// Decode a complete geo-query at a request boundary.
pub fn decode_geo_query(
    georel: String,
    geometry_type: String,
    coordinates: serde_json::Value,
    geoproperty: Option<NgsiLdReference>,
) -> Result<GeoQueryInput, GeoQueryDecodeError> {
    GeoQueryInput::from_wire(georel, geometry_type, coordinates, geoproperty).map_err(GeoQueryDecodeError::from)
}

/// Geo-query decoding failure at a request boundary.
#[derive(Debug, thiserror::Error)]
pub enum GeoQueryDecodeError {
    /// The relationship token is invalid.
    #[error("invalid georel: {0}")]
    Decode(#[from] andromeda_ngsi_ld_query::geo::input::GeoQueryInputError),
}

/// Deserialize an optional comma-separated entity identifier list.
///
/// NGSI-LD clause 4.6.2 admits only URNs and URLs as entity identifiers. Empty
/// members are malformed input, so the request is rejected while extracting the
/// query rather than leaving a later compiler to reinterpret the wire value.
pub fn deserialize_optional_uri_csv<'de, D>(deserializer: D) -> Result<Option<Vec<Uri>>, D::Error>
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
                    return Err(serde::de::Error::custom("entity identifier list contains an empty member"));
                }
                member.parse::<Uri>().map_err(serde::de::Error::custom)
            })
            .collect()
    })
    .transpose()
}

/// Deserialize an optional non-empty `idPattern` as a compiled regular expression.
pub fn deserialize_optional_pattern<'de, D>(deserializer: D) -> Result<Option<Pattern>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = Option::<String>::deserialize(deserializer)?;
    raw.map(|value| {
        if value.trim().is_empty() {
            return Err(serde::de::Error::custom("Invalid idPattern: empty value"));
        }
        value.parse().map_err(serde::de::Error::custom)
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
    raw.map(|value| parser::parse(&value).map_err(serde::de::Error::custom)).transpose()
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
                    return Err(serde::de::Error::custom("attribute name list contains an empty member"));
                }
                member.parse().map_err(serde::de::Error::custom)
            })
            .collect()
    })
    .transpose()
}

/// Deserialize an optional comma-separated list of dataset selectors.
///
/// Clause 4.5.5 defines `datasetId` as a list of URIs. Empty entries and
/// non-URI values are malformed request data at the extraction boundary.
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
                    return Err(serde::de::Error::custom("dataset identifier list contains an empty member"));
                }
                member.parse().map_err(serde::de::Error::custom)
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
pub fn deserialize_optional_dataset_selector<'de, D>(deserializer: D) -> Result<Option<DatasetSelector>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = Option::<String>::deserialize(deserializer)?;
    raw.map(|value| {
        let value = value.trim();
        if value.contains(',') {
            return Err(serde::de::Error::custom("datasetId must contain one URI or @none selector"));
        }
        if value.is_empty() {
            return Err(serde::de::Error::custom("datasetId must not be empty"));
        }
        value.parse().map_err(serde::de::Error::custom)
    })
    .transpose()
}

/// Deserialize an optional ISO-8601 calendar duration at the request boundary.
pub fn deserialize_optional_iso8601_duration<'de, D>(deserializer: D) -> Result<Option<Iso8601Duration>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = Option::<String>::deserialize(deserializer)?;
    raw.map(|value| value.parse().map_err(|error| serde::de::Error::custom(format!("{error:?}"))))
        .transpose()
}
