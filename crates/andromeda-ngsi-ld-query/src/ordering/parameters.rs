//! The complete ordering definition carried by a clause-5.2.23 `Query`.

use crate::ordering::{
    collation::IcuCollation,
    geometry::{OrderingGeometry, OrderingGeometryError},
    term::OrderTerm,
};
use andromeda_ngsi_ld_geometry::{geometry::Geometry, geometry_type::GeometryType};
use serde::{Deserialize, Deserializer, Serialize, de::Error as DeserializeError};
use serde_json::Value;
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
    #[serde(alias = "orderFrom", default, deserialize_with = "deserialize_optional_coordinates")]
    coordinates: Option<Value>,
    /// Geometry type, defaulting to Point when coordinates are present.
    #[serde(alias = "orderGeometry", default)]
    geometry: Option<GeometryType>,
    /// Ordering terms.
    #[serde(rename = "orderBy", default, deserialize_with = "deserialize_order_by")]
    order_by: Vec<OrderTerm>,
}

/// Parse optional coordinates, decoding query-string JSON exactly once.
fn deserialize_optional_coordinates<'de, D>(deserializer: D) -> Result<Option<Value>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<Value>::deserialize(deserializer)?
        .map(|value| match value {
            Value::String(raw) => serde_json::from_str(&raw).map_err(DeserializeError::custom),
            other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_) | Value::Object(_)) => Ok(other),
        })
        .transpose()
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
            (None, Some(_)) => return Err(DeserializeError::custom("orderGeometry requires orderFrom")),
            (Some(coordinates), geometry) => {
                let geometry_type = geometry.unwrap_or(GeometryType::Point);
                let geometry = Geometry::from_type_and_coordinates(geometry_type, coordinates)
                    .map_err(|error| DeserializeError::custom(OrderingGeometryError::Geometry(error)))?;
                Some(OrderingGeometry::new(geometry))
            }
        };
        Ok(OrderingParameters {
            collation: wire.collation,
            geometry,
            order_by: wire.order_by,
        })
    }
}

/// Deserialize the JSON array defined by Table 5.2.43-1 or the CSV string used
/// by the HTTP GET binding into the same typed ordering terms.
fn deserialize_order_by<'de, D>(deserializer: D) -> Result<Vec<OrderTerm>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    match value {
        None => Ok(Vec::new()),
        Some(Value::String(raw)) => raw
            .split(',')
            .map(str::trim)
            .map(|term| term.parse().map_err(DeserializeError::custom))
            .collect(),
        Some(Value::Array(values)) => values
            .into_iter()
            .map(|value| serde_json::from_value(value).map_err(DeserializeError::custom))
            .collect(),
        Some(Value::Null | Value::Bool(_) | Value::Number(_) | Value::Object(_)) => {
            Err(DeserializeError::custom("orderBy must be a string array or comma-separated string"))
        }
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
