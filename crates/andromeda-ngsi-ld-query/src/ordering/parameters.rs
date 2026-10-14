//! The complete ordering definition carried by a clause-5.2.23 `Query`.

use crate::{
    error::QueryError,
    ordering::{
        collation::IcuCollation,
        geometry::{OrderingGeometry, OrderingGeometryError},
        term::OrderTerm,
    },
};
use andromeda_ngsi_ld_geometry::{geometry_type::GeometryType, query_coordinates::QueryCoordinates};
use serde::{
    Deserialize,
    Deserializer,
    Serialize,
    de::{Error as DeserializeError, SeqAccess, Visitor},
};
use std::fmt;
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
    #[serde(rename = "orderBy", default, deserialize_with = "deserialize_order_by")]
    pub order_by: Vec<OrderTerm>,
}

/// Wire members shared by the JSON body and query-string bindings.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OrderingParametersWire {
    /// ICU collation.
    #[serde(default)]
    collation: Option<IcuCollation>,
    /// Geometry coordinates, or JSON text carrying them in a query string.
    #[serde(alias = "orderFrom", default)]
    coordinates: Option<QueryCoordinates>,
    /// Geometry type, defaulting to Point when coordinates are present.
    #[serde(alias = "orderGeometry", default)]
    geometry: Option<GeometryType>,
    /// Ordering terms.
    #[serde(rename = "orderBy", default, deserialize_with = "deserialize_order_by")]
    order_by: Vec<OrderTerm>,
}

// A derive cannot combine the optional geometry type and coordinates into the
// validated Geometry invariant while also accepting their GET aliases.
impl<'de> Deserialize<'de> for OrderingParameters {
    fn deserialize<D>(deserializer: D) -> Result<OrderingParameters, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = OrderingParametersWire::deserialize(deserializer)?;
        let geometry = match (wire.coordinates, wire.geometry) {
            (None, None) => None,
            (None, Some(_)) => return Err(DeserializeError::custom(OrderingGeometryError::WithoutCoordinates)),
            (Some(coordinates), geometry) => {
                let geometry_type = geometry.unwrap_or(GeometryType::Point);
                let written = coordinates.into_written().map_err(DeserializeError::custom)?;
                let geometry = written
                    .read_as(geometry_type)
                    .map_err(|error| DeserializeError::custom(OrderingGeometryError::Geometry(error)))?;
                Some(OrderingGeometry::new(geometry))
            }
        };
        // Table 6.4.3.2-1 makes `orderFrom` mandatory when `orderBy` orders by
        // distance, so a geo-query's geometry never stands in for it.
        if geometry.is_none() && wire.order_by.iter().any(|term| term.direction.is_distance()) {
            return Err(DeserializeError::custom(QueryError::DistanceOrderWithoutReference));
        }
        Ok(OrderingParameters {
            collation: wire.collation,
            geometry,
            order_by: wire.order_by,
        })
    }
}

/// Reads the JSON array of Table 5.2.43-1 or the CSV string of the HTTP GET binding into the same ordering terms.
fn deserialize_order_by<'de, D>(deserializer: D) -> Result<Vec<OrderTerm>, D::Error>
where
    D: Deserializer<'de>,
{
    deserializer.deserialize_any(OrderByVisitor)
}

/// Reads the `orderBy` member as the terms it spells, never as a JSON tree.
struct OrderByVisitor;

impl<'de> Visitor<'de> for OrderByVisitor {
    type Value = Vec<OrderTerm>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("orderBy must be a string array or comma-separated string")
    }

    fn visit_unit<E: DeserializeError>(self) -> Result<Vec<OrderTerm>, E> {
        Ok(Vec::new())
    }

    fn visit_none<E: DeserializeError>(self) -> Result<Vec<OrderTerm>, E> {
        Ok(Vec::new())
    }

    fn visit_str<E: DeserializeError>(self, terms: &str) -> Result<Vec<OrderTerm>, E> {
        terms
            .split(',')
            .map(str::trim)
            .map(|term| term.parse().map_err(DeserializeError::custom))
            .collect()
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut terms: A) -> Result<Vec<OrderTerm>, A::Error> {
        let mut read = Vec::with_capacity(terms.size_hint().unwrap_or(0));
        while let Some(term) = terms.next_element()? {
            read.push(term);
        }
        Ok(read)
    }
}

#[cfg(test)]
mod tests {
    use crate::ordering::{parameters::OrderingParameters, term::OrderDirection};
    use andromeda_ngsi_ld_geometry::geometry_type::GeometryType;

    #[test]
    fn a_point_ordering_deserializes_to_typed_geometry() {
        let ordering: OrderingParameters = serde_json::from_value(serde_json::json!({
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
        let body: OrderingParameters = serde_json::from_value(serde_json::json!({
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
            serde_json::from_value::<OrderingParameters>(serde_json::json!({
                "orderBy": ["location;dist-asc"],
                "coordinates": "not coordinates"
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<OrderingParameters>(serde_json::json!({
                "orderBy": ["location;dist-asc"],
                "geometry": "GeometryCollection",
                "coordinates": []
            }))
            .is_err()
        );
    }
}
