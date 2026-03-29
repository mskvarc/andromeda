use crate::types::Value;
use geojson::{Geometry, Value as GeoJsonValue};

impl Value {
    fn try_as_geometry(&self) -> Option<Geometry> {
        match self {
            Value::Geospatial(g) => Some((**g).clone()),
            Value::String(s) => serde_json::from_str(s).ok(),
            _ => None,
        }
    }

    fn as_coords_2d(&self) -> Option<Vec<f64>> {
        match self {
            Value::Array(arr) if arr.len() >= 2 => {
                let x = arr[0].to_float();
                let y = arr[1].to_float();
                Some(vec![x, y])
            }
            _ => None,
        }
    }

    fn as_coords_list(&self) -> Option<Vec<Vec<f64>>> {
        match self {
            Value::Array(arr) => {
                let mut coords = Vec::new();
                for item in arr {
                    if let Some(pt) = item.as_coords_2d() {
                        coords.push(pt);
                    }
                }
                if coords.is_empty() { None } else { Some(coords) }
            }
            _ => None,
        }
    }

    pub fn to_point(&self) -> Geometry {
        self.try_as_geometry().filter(|g| matches!(g.value, GeoJsonValue::Point(_))).unwrap_or_else(|| {
            if let Some(c) = self.as_coords_2d() {
                Geometry::new(GeoJsonValue::Point(c))
            } else {
                Geometry::new(GeoJsonValue::Point(vec![0.0, 0.0]))
            }
        })
    }

    pub fn to_multipoint(&self) -> Geometry {
        self.try_as_geometry()
            .filter(|g| matches!(g.value, GeoJsonValue::MultiPoint(_)))
            .unwrap_or_else(|| {
                if let Some(coords) = self.as_coords_list() {
                    Geometry::new(GeoJsonValue::MultiPoint(coords))
                } else {
                    Geometry::new(GeoJsonValue::MultiPoint(vec![]))
                }
            })
    }

    pub fn to_linestring(&self) -> Geometry {
        self.try_as_geometry()
            .filter(|g| matches!(g.value, GeoJsonValue::LineString(_)))
            .unwrap_or_else(|| {
                if let Some(coords) = self.as_coords_list() {
                    Geometry::new(GeoJsonValue::LineString(coords))
                } else {
                    Geometry::new(GeoJsonValue::LineString(vec![]))
                }
            })
    }

    pub fn to_multilinestring(&self) -> Geometry {
        self.try_as_geometry()
            .filter(|g| matches!(g.value, GeoJsonValue::MultiLineString(_)))
            .unwrap_or_else(|| Geometry::new(GeoJsonValue::MultiLineString(vec![])))
    }

    pub fn to_polygon(&self) -> Geometry {
        self.try_as_geometry()
            .filter(|g| matches!(g.value, GeoJsonValue::Polygon(_)))
            .unwrap_or_else(|| {
                if let Some(coords) = self.as_coords_list() {
                    Geometry::new(GeoJsonValue::Polygon(vec![coords]))
                } else {
                    Geometry::new(GeoJsonValue::Polygon(vec![]))
                }
            })
    }

    pub fn to_multipolygon(&self) -> Geometry {
        self.try_as_geometry()
            .filter(|g| matches!(g.value, GeoJsonValue::MultiPolygon(_)))
            .unwrap_or_else(|| Geometry::new(GeoJsonValue::MultiPolygon(vec![])))
    }
}
