//! The complete ordering definition carried by a clause-5.2.23 `Query` or a clause 6.4.3.2 query string.

use crate::{
    comma_separated_list::{ListedItem, parse_listed},
    defined_token::deserialize_optional_token,
    error::QueryError,
    ordering::{
        collation::IcuCollation,
        geometry::{OrderingGeometry, OrderingGeometryError},
        term::OrderTerm,
    },
};
use andromeda_ngsi_ld_geometry::{geometry_type::GeometryType, query_coordinates::QueryCoordinates};
use serde::{Deserialize, Deserializer, Serialize, de::Error as DeserializeError};
use thiserror::Error;
use typed_builder::TypedBuilder;

/// Parameters that order entity results (ETSI GS CIM 009 v1.9.1 clause 5.2.43).
#[derive(Debug, Clone, Default, PartialEq, Serialize, TypedBuilder)]
#[serde(rename_all = "camelCase")]
#[builder(field_defaults(default = Default::default()))]
pub struct OrderingParameters {
    /// ICU collation applied to string-valued ordering members.
    #[serde(default)]
    pub collation: Option<IcuCollation>,
    /// Reference geometry used by distance ordering terms.
    #[serde(flatten, default)]
    pub geometry: Option<OrderingGeometry>,
    /// Members and directions that define the result order.
    #[serde(rename = "orderBy", default)]
    pub order_by: Vec<OrderTerm>,
}

/// A malformed ordering definition, whichever binding carried it.
#[derive(Debug, Error)]
pub enum OrderingParametersError {
    /// The reference geometry is malformed or was sent without its coordinates.
    #[error(transparent)]
    Geometry(#[from] OrderingGeometryError),
    /// A distance term names no reference geometry to measure from.
    #[error(transparent)]
    Query(#[from] QueryError),
}

/// The `ordering` member of a clause 5.2.23 Query body (Table 5.2.43-1), whose
/// `orderBy` is a JSON array of terms.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OrderingBody {
    /// ICU collation.
    #[serde(default)]
    collation: Option<IcuCollation>,
    /// `GeoJSON` coordinates of the reference geometry.
    #[serde(default)]
    coordinates: Option<QueryCoordinates>,
    /// `GeoJSON` geometry type, defaulting to Point when coordinates are present.
    #[serde(default)]
    geometry: Option<GeometryType>,
    /// Ordering terms, one string per term.
    #[serde(rename = "orderBy", default)]
    order_by: Vec<OrderTerm>,
}

impl TryFrom<OrderingBody> for OrderingParameters {
    type Error = OrderingParametersError;

    fn try_from(body: OrderingBody) -> Result<OrderingParameters, OrderingParametersError> {
        OrderingParameters::assemble(body.collation, body.coordinates, body.geometry, body.order_by)
    }
}

/// The ordering parameters of a GET query string (clause 6.4.3.2 and Table 6.4.3.2-1): `orderBy` is one
/// comma-separated string, and the reference geometry is `orderFrom` as JSON text with an optional `orderGeometry`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OrderingQueryString {
    /// ICU collation.
    #[serde(default)]
    collation: Option<IcuCollation>,
    /// `GeoJSON` coordinates of the reference geometry, as JSON text.
    #[serde(default)]
    order_from: Option<QueryCoordinates>,
    /// `GeoJSON` geometry type, defaulting to Point when `orderFrom` is present.
    #[serde(default, deserialize_with = "deserialize_optional_token")]
    order_geometry: Option<GeometryType>,
    /// Ordering terms, comma-separated.
    #[serde(default, deserialize_with = "deserialize_listed_order_by")]
    order_by: Vec<OrderTerm>,
}

// The flattened query parameters read ordering through this impl, so it is the GET binding. The JSON
// `ordering` member reads `OrderingBody` instead: the two bindings spell `orderBy` and the geometry
// differently, and a derive cannot accept both spellings of the same field.
impl<'de> Deserialize<'de> for OrderingParameters {
    fn deserialize<D>(deserializer: D) -> Result<OrderingParameters, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = OrderingQueryString::deserialize(deserializer)?;
        OrderingParameters::assemble(wire.collation, wire.order_from, wire.order_geometry, wire.order_by).map_err(DeserializeError::custom)
    }
}

impl OrderingParameters {
    /// Joins the parts either binding reads into one ordering, validating the reference geometry and checking
    /// that a distance term has one to measure from.
    fn assemble(
        collation: Option<IcuCollation>,
        coordinates: Option<QueryCoordinates>,
        geometry_type: Option<GeometryType>,
        order_by: Vec<OrderTerm>,
    ) -> Result<OrderingParameters, OrderingParametersError> {
        let geometry = match (coordinates, geometry_type) {
            (None, None) => None,
            (None, Some(_)) => return Err(OrderingGeometryError::WithoutCoordinates.into()),
            (Some(coordinates), geometry_type) => {
                let geometry_type = geometry_type.unwrap_or(GeometryType::Point);
                let written = coordinates.into_written().map_err(OrderingGeometryError::Coordinates)?;
                let geometry = written.read_as(geometry_type).map_err(OrderingGeometryError::Geometry)?;
                Some(OrderingGeometry::new(geometry))
            }
        };
        // Table 6.4.3.2-1 makes `orderFrom` mandatory when `orderBy` orders by
        // distance, so a geo-query's geometry never stands in for it.
        if geometry.is_none() && order_by.iter().any(|term| term.direction.is_distance()) {
            return Err(QueryError::DistanceOrderWithoutReference.into());
        }
        Ok(OrderingParameters { collation, geometry, order_by })
    }
}

/// Reads the GET binding's `orderBy` as the comma-separated terms it spells (Table 6.4.3.2-1).
fn deserialize_listed_order_by<'de, D>(deserializer: D) -> Result<Vec<OrderTerm>, D::Error>
where
    D: Deserializer<'de>,
{
    let terms = String::deserialize(deserializer)?;
    parse_listed(&terms, ListedItem::OrderingTerm).map_err(DeserializeError::custom)
}

#[cfg(test)]
mod tests {
    use crate::ordering::{
        parameters::{OrderingBody, OrderingParameters},
        term::OrderDirection,
    };
    use andromeda_ngsi_ld_geometry::geometry_type::GeometryType;

    /// Reads a Query body `ordering` member: the JSON binding, whose `orderBy` is an array.
    fn from_body(value: serde_json::Value) -> Result<OrderingParameters, String> {
        let body: OrderingBody = serde_json::from_value(value).map_err(|error| error.to_string())?;
        OrderingParameters::try_from(body).map_err(|error| error.to_string())
    }

    #[test]
    fn a_point_ordering_deserializes_to_typed_geometry() {
        let ordering = from_body(serde_json::json!({
            "orderBy": ["location;dist-asc"],
            "coordinates": [13.4, 52.5]
        }))
        .expect("valid ordering");
        pretty_assertions::assert_eq!(ordering.order_by[0].direction, OrderDirection::DistAsc);
        let geometry = ordering.geometry.expect("reference geometry").geometry.to_geojson();
        pretty_assertions::assert_eq!(geometry.value.type_name(), GeometryType::Point.to_string());
    }

    /// Table 5.2.43-1 and the GET binding: `orderFrom` carries JSON text and `orderBy` a comma-separated list.
    #[test]
    fn the_query_string_spelling_reads_as_the_body_spelling() {
        let ordering: OrderingParameters = serde_json::from_value(serde_json::json!({
            "orderBy": "location;dist-asc, name;asc",
            "orderFrom": "[13.4, 52.5]"
        }))
        .expect("valid ordering");
        pretty_assertions::assert_eq!(ordering.order_by.len(), 2);
        let body = from_body(serde_json::json!({
            "orderBy": ["location;dist-asc", "name;asc"],
            "coordinates": [13.4, 52.5]
        }))
        .expect("valid ordering");
        pretty_assertions::assert_eq!(ordering, body);
    }

    /// Table 6.4.3.2-1: `orderFrom` "shall be one if orderBy uses order by
    /// distance", whichever term carries the distance direction.
    #[test]
    fn a_distance_ordering_without_a_reference_geometry_is_rejected() {
        for order_by in ["location;dist-asc", "location;dist-desc", "name;asc,location;dist-asc"] {
            assert!(
                serde_json::from_value::<OrderingParameters>(serde_json::json!({ "orderBy": order_by })).is_err(),
                "{order_by}"
            );
            assert!(from_body(serde_json::json!({ "orderBy": [order_by] })).is_err(), "{order_by}");
        }
    }

    #[test]
    fn a_value_ordering_needs_no_reference_geometry() {
        let ordering: OrderingParameters = serde_json::from_value(serde_json::json!({ "orderBy": "name;desc,location" })).expect("valid ordering");
        assert!(ordering.geometry.is_none());
    }

    #[test]
    fn malformed_coordinates_and_geometry_collections_are_rejected() {
        assert!(
            from_body(serde_json::json!({
                "orderBy": ["location;dist-asc"],
                "coordinates": "not coordinates"
            }))
            .is_err()
        );
        assert!(
            from_body(serde_json::json!({
                "orderBy": ["location;dist-asc"],
                "geometry": "GeometryCollection",
                "coordinates": []
            }))
            .is_err()
        );
    }

    /// The JSON body spells the reference geometry `coordinates` and `geometry`; the GET binding spells it
    /// `orderFrom` and `orderGeometry`, so each binding refuses the other's names.
    #[test]
    fn the_query_string_refuses_the_body_spelling_of_the_geometry() {
        for member in [
            serde_json::json!({"orderBy": "location;dist-asc", "coordinates": [13.4, 52.5]}),
            serde_json::json!({"orderBy": "location;dist-asc", "orderFrom": "[13.4, 52.5]", "geometry": "Point"}),
        ] {
            assert!(serde_json::from_value::<OrderingParameters>(member.clone()).is_err(), "{member}");
        }
    }

    #[test]
    fn the_body_refuses_the_query_string_spelling() {
        for member in [
            serde_json::json!({"orderBy": ["location;dist-asc"], "orderFrom": "[13.4, 52.5]"}),
            serde_json::json!({"orderBy": ["location;dist-asc"], "coordinates": [13.4, 52.5], "orderGeometry": "Point"}),
            serde_json::json!({"orderBy": "name;asc"}),
        ] {
            assert!(from_body(member.clone()).is_err(), "{member}");
        }
    }

    #[test]
    fn the_body_orders_by_an_array_of_terms_and_refuses_a_bang_prefix() {
        assert!(from_body(serde_json::json!({"orderBy": ["!name"]})).is_err());
        assert!(from_body(serde_json::json!({"orderBy": ["name;desc"]})).is_ok());
    }
}
