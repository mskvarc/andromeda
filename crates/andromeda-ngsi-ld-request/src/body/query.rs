//! POST query request body types for the NGSI-LD Entity query API.

use crate::{
    error::ParameterError,
    query::deserialization::{decode_geo_query, deserialize_optional_filter_expr},
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, reference::NgsiLdReference, uri::Uri};
use andromeda_ngsi_ld_query::{filter::expression::Expr, geo::input::GeoQueryInput, pattern::Pattern, type_selection::grammar::TypeExprRaw};
use serde::Deserialize;
use serde_json::Value;

/// POST query request body (NGSI-LD 6.4.3.2).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryRequest {
    /// The request-body resource type, which clause 5.2.23 fixes as `Query`.
    /// Entity type filtering is separate, carried by `entities[].type`.
    #[serde(rename = "type")]
    pub type_: Option<String>,
    /// Entity selectors constraining the query.
    pub entities: Option<Vec<EntitySelection>>,
    /// Attributes the response is narrowed to, comma-separated (`attrs`).
    pub attrs: Option<Vec<AttributeName>>,
    /// The `q` filter expression (clause 4.9), parsed at the request boundary.
    #[serde(default, deserialize_with = "deserialize_optional_filter_expr")]
    pub q: Option<Expr>,
    /// The geo-query, as the `georel`/`geometry`/`coordinates` triple.
    pub geo_q: Option<GeoQueryRequest>,
    /// The page size (clause 5.5.9 `limit`).
    pub limit: Option<u32>,
    /// How many results to skip before the page (`offset`).
    pub offset: Option<u32>,
    /// Whether the response reports the total number of matches (`count`).
    pub count: Option<bool>,
}

impl QueryRequest {
    /// Check the body's resource type is the clause-5.2.23 `Query`.
    ///
    /// An absent type is accepted, for compatibility with clients that omit it;
    /// only a type naming something other than `Query` is rejected, rather than
    /// silently reinterpreted as a query.
    ///
    /// # Errors
    ///
    /// Returns [`ParameterError::RequestTypeInvalid`] when the body names a type
    /// that is not `Query`.
    pub fn validate_type(&self) -> Result<(), ParameterError> {
        match self.type_.as_deref() {
            None | Some("Query") => Ok(()),
            Some(other) => Err(ParameterError::RequestTypeInvalid(other.to_string())),
        }
    }
}

/// Entity selector within a POST query body.
///
/// Supports both the full object form (`{"id": "...", "type": "..."}`) and the
/// short string form (just an entity ID).
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum EntitySelection {
    /// Full object form: `{"id": "...", "type": "...", "idPattern": "..."}`.
    Object {
        /// Entity identifiers to select, comma-separated (clause 6.3.x `id`).
        id: Option<Uri>,
        /// A regular expression the selected identifiers must match (`idPattern`).
        #[serde(rename = "idPattern")]
        #[serde(default, deserialize_with = "crate::query::deserialization::deserialize_optional_pattern")]
        id_pattern: Option<Pattern>,
        /// The parsed entity type-selection expression.
        #[serde(rename = "type")]
        type_: Option<TypeExprRaw>,
    },
    /// Short form: just an entity ID string.
    Id(Uri),
}

impl EntitySelection {
    /// Returns the entity ID, if present.
    pub const fn id(&self) -> Option<&Uri> {
        match self {
            EntitySelection::Object { id, .. } => id.as_ref(),
            EntitySelection::Id(id) => Some(id),
        }
    }

    /// Returns the ID pattern, if present.
    pub const fn id_pattern(&self) -> Option<&Pattern> {
        match self {
            EntitySelection::Object { id_pattern, .. } => id_pattern.as_ref(),
            EntitySelection::Id(_) => None,
        }
    }

    /// Returns the entity type, if present.
    pub const fn type_(&self) -> Option<&TypeExprRaw> {
        match self {
            EntitySelection::Object { type_, .. } => type_.as_ref(),
            EntitySelection::Id(_) => None,
        }
    }
}

/// Geo-query filter within a POST query body.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeoQueryWire {
    /// The spatial relationship of the geo-query (clause 4.10 `georel`).
    georel: String,
    /// The GeoJSON geometry type of the geo-query's reference (`geometry`).
    geometry: String,
    /// The coordinates of the geo-query's reference geometry (`coordinates`).
    coordinates: Value,
    /// The GeoProperty the geo-query applies to (`geoproperty`).
    geoproperty: Option<NgsiLdReference>,
}

/// Complete geo-query parsed while deserializing a POST body.
#[derive(Debug)]
pub struct GeoQueryRequest(pub GeoQueryInput);

impl<'de> Deserialize<'de> for GeoQueryRequest {
    fn deserialize<D>(deserializer: D) -> Result<GeoQueryRequest, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = GeoQueryWire::deserialize(deserializer)?;
        decode_geo_query(wire.georel, wire.geometry, wire.coordinates, wire.geoproperty)
            .map(GeoQueryRequest)
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use crate::body::query::QueryRequest;

    /// A query body carrying only the given resource type.
    fn body_with_type(type_: Option<&str>) -> QueryRequest {
        QueryRequest {
            type_: type_.map(str::to_string),
            entities: None,
            attrs: None,
            q: None,
            geo_q: None,
            limit: None,
            offset: None,
            count: None,
        }
    }

    #[test]
    fn a_query_body_typed_query_is_accepted() {
        assert!(body_with_type(Some("Query")).validate_type().is_ok());
    }

    #[test]
    fn a_query_body_without_a_type_is_accepted() {
        assert!(body_with_type(None).validate_type().is_ok());
    }

    #[test]
    fn a_query_body_typed_anything_else_is_rejected() {
        // Clause 5.2.23 fixes the body type as `Query`; a `Subscription` body
        // posted to the query endpoint is a malformed request, not a query.
        assert!(body_with_type(Some("Subscription")).validate_type().is_err());
    }
}
