//! Query parameters for the NGSI-LD Entity query API (5.7.2).

use crate::query::{error::QueryError, params::is_valid_uri};
use serde::Deserialize;

/// Raw HTTP query parameters for entity queries, deserialized from the request URL.
///
/// Corresponds to the query parameters defined in NGSI-LD 6.3.5 (Query Entities)
/// and related operations. All values are in their raw (compact) form before
/// context expansion.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntityQueryParameters {
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub entity_type: Option<String>,
    #[serde(rename = "idPattern")]
    pub id_pattern: Option<String>,
    pub attrs: Option<String>,
    pub q: Option<String>,
    pub georel: Option<String>,
    pub geometry: Option<String>,
    pub coordinates: Option<String>,
    pub geoproperty: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub count: Option<bool>,
    pub options: Option<String>,
    #[serde(rename = "local")]
    pub local: Option<bool>,
    pub pick: Option<String>,
    pub omit: Option<String>,
    #[serde(rename = "orderBy")]
    pub order_by: Option<String>,
    #[serde(rename = "scopeQ")]
    pub scope_q: Option<String>,
    /// Parsed by the [`LanguageNegotiation`] extractor at the HTTP layer; retained
    /// here so `deny_unknown_fields` accepts `?lang=...` in the query string.
    #[allow(dead_code)]
    pub lang: Option<String>,
    #[serde(rename = "datasetId")]
    pub dataset_id: Option<String>,
    #[serde(rename = "expandValues")]
    pub expand_values: Option<String>,
    #[serde(rename = "jsonKeys")]
    pub json_keys: Option<String>,
    pub format: Option<String>,
    pub join: Option<String>,
    #[serde(rename = "joinLevel")]
    pub join_level: Option<u32>,
    #[serde(rename = "containedBy")]
    pub contained_by: Option<String>,
    #[serde(rename = "entityMap")]
    pub entity_map: Option<bool>,
    #[serde(rename = "entityMapLifetime")]
    pub entity_map_lifetime: Option<String>,
    #[serde(rename = "splitEntities")]
    pub split_entities: Option<bool>,
    #[serde(rename = "geometryProperty")]
    pub geometry_property: Option<String>,
    /// ICU collation identifier for orderBy string sorting (S4.23).
    pub collation: Option<String>,
    /// Reference coordinates for distance-based ordering (S5.2.43).
    #[serde(rename = "orderFrom")]
    pub order_from: Option<String>,
    /// Geometry type for orderFrom coordinates (S5.2.43). Defaults to "Point".
    #[serde(rename = "orderGeometry")]
    pub order_geometry: Option<String>,
}

impl_shared_query_params!(EntityQueryParameters {
    id: id,
    attrs: attrs,
    pick: pick,
    omit: omit,
    dataset_id: dataset_id,
    limit: limit,
    offset: offset
});

impl EntityQueryParameters {
    /// Parses the `orderBy` parameter into a list of ordering terms.
    ///
    /// Supports two syntaxes per NGSI-LD S4.23:
    /// - Semicolon-delimited: `"attrName;desc"`, `"location;dist-asc"`
    /// - Legacy bang prefix: `"!attrName"` (equivalent to desc)
    ///
    /// Terms with unrecognized direction values are silently skipped.
    /// Defaults to ascending when no direction is specified.
    pub fn parse_order_by(&self) -> Vec<OrderTerm> {
        self.order_by
            .as_deref()
            .map(|s| {
                s.split(',')
                    .filter_map(|term| {
                        let term = term.trim();
                        if term.is_empty() {
                            return None;
                        }
                        // Spec S4.23.3: AttrName *1(;directionOp)
                        // directionOp ::= asc | desc | dist-asc | dist-desc
                        if let Some((attr, dir_str)) = term.split_once(';') {
                            let attr = attr.trim();
                            let direction = match dir_str.trim() {
                                "asc" => OrderDirection::Asc,
                                "desc" => OrderDirection::Desc,
                                "dist-asc" => OrderDirection::DistAsc,
                                "dist-desc" => OrderDirection::DistDesc,
                                _ => return None,
                            };
                            Some(OrderTerm {
                                attr: attr.to_string(),
                                direction,
                            })
                        } else if let Some(attr) = term.strip_prefix('!') {
                            // Legacy shorthand: !attr means desc
                            Some(OrderTerm {
                                attr: attr.to_string(),
                                direction: OrderDirection::Desc,
                            })
                        } else {
                            Some(OrderTerm {
                                attr: term.to_string(),
                                direction: OrderDirection::Asc,
                            })
                        }
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Parses the `options` and `format` parameters into structured flags.
    ///
    /// Handles both the legacy `options` parameter (e.g. `"keyValues,sysAttrs"`)
    /// and the newer `format` parameter, with `format` taking precedence where
    /// both are provided.
    pub fn parse_options(&self) -> EntityQueryParametersOptions {
        let opts = self.options.as_deref().unwrap_or("");
        let fmt = self.format.as_deref().unwrap_or("");
        EntityQueryParametersOptions {
            key_values: fmt == "keyValues" || fmt == "simplified" || opts.contains("keyValues") || opts.contains("simplified"),
            sys_attrs: opts.contains("sysAttrs"),
            count: self.count.unwrap_or(false),
        }
    }

    /// Validate query parameters.
    pub fn validate(&self) -> Result<(), QueryError> {
        // Validate options parameter
        if let Some(opts) = &self.options {
            for opt in opts.split(',') {
                let opt = opt.trim();
                if !matches!(opt, "keyValues" | "simplified" | "sysAttrs" | "concise" | "normalized") {
                    return Err(QueryError::InvalidRequest(format!("Invalid options value: '{}'", opt)));
                }
            }
        }

        if let Some(fmt) = &self.format
            && !matches!(fmt.as_str(), "normalized" | "keyValues" | "simplified" | "concise")
        {
            return Err(QueryError::InvalidRequest(format!("Invalid format value: '{}'", fmt)));
        }

        if let Some(ids) = &self.id {
            for id in ids.split(',').map(str::trim) {
                if id.is_empty() {
                    return Err(QueryError::InvalidRequest("Invalid id: empty value in comma-separated list".to_string()));
                }
                if !is_valid_uri(id) {
                    return Err(QueryError::BadRequestData(format!("Invalid id value: '{}'", id)));
                }
            }
        }

        if let Some(pattern) = &self.id_pattern
            && pattern.trim().is_empty()
        {
            return Err(QueryError::BadRequestData("Invalid idPattern: empty value".to_string()));
        }

        // Validate orderGeometry: must be a valid GeoJSON geometry type, not GeometryCollection (S5.2.43)
        if let Some(ref order_geometry) = self.order_geometry {
            match order_geometry.as_str() {
                "Point" | "MultiPoint" | "LineString" | "MultiLineString" | "Polygon" | "MultiPolygon" => {}
                "GeometryCollection" => {
                    return Err(QueryError::BadRequestData("orderGeometry does not support GeometryCollection".to_string()));
                }
                other => {
                    return Err(QueryError::BadRequestData(format!("Invalid orderGeometry value: '{}'", other)));
                }
            }
        }

        // Validate orderFrom: must be valid JSON coordinates (S5.2.43)
        if let Some(ref order_from) = self.order_from {
            serde_json::from_str::<serde_json::Value>(order_from)
                .map_err(|_| QueryError::BadRequestData(format!("Invalid orderFrom value: '{}' is not valid JSON", order_from)))?;
        }

        Ok(())
    }
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

/// Parsed response shaping options from the `options` and `format` parameters.
#[derive(Debug, Default)]
pub struct EntityQueryParametersOptions {
    /// When true, return entities in simplified key-values representation.
    pub key_values: bool,
    /// When true, include system-generated attributes (`createdAt`, `modifiedAt`).
    pub sys_attrs: bool,
    /// When true, include a total count of matching entities in the response.
    pub count: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::params::SharedQueryParams;

    fn params_from_query(query: &str) -> EntityQueryParameters {
        params_from_query!(query, EntityQueryParameters)
    }

    fn parse_types(p: &EntityQueryParameters) -> Vec<String> {
        p.entity_type
            .as_deref()
            .map(|s| s.split(',').map(|t| t.trim().to_string()).collect())
            .unwrap_or_default()
    }

    #[test]
    fn parse_ids_splits_comma_separated() {
        let p = params_from_query("id=urn:a,urn:b,urn:c");
        let ids = p.parse_ids().unwrap();
        assert_eq!(ids, vec!["urn:a", "urn:b", "urn:c"]);
    }

    #[test]
    fn parse_types_splits_comma_separated() {
        let p = params_from_query("type=Vehicle,Building");
        let types = parse_types(&p);
        assert_eq!(types, vec!["Vehicle", "Building"]);
    }

    #[test]
    fn parse_types_empty_when_absent() {
        let p = params_from_query("");
        assert!(parse_types(&p).is_empty());
    }

    #[test]
    fn parse_attrs_splits_comma_separated() {
        let p = params_from_query("attrs=speed,temperature");
        let attrs = p.parse_attrs();
        assert_eq!(attrs, vec!["speed", "temperature"]);
    }

    #[test]
    fn parse_order_by_asc_default() {
        let p = params_from_query("orderBy=temperature");
        let order = p.parse_order_by();
        assert_eq!(order.len(), 1);
        assert_eq!(order[0].attr, "temperature");
        assert_eq!(order[0].direction, OrderDirection::Asc);
    }

    #[test]
    fn parse_order_by_desc_with_bang() {
        let p = params_from_query("orderBy=!temperature");
        let order = p.parse_order_by();
        assert_eq!(order.len(), 1);
        assert_eq!(order[0].attr, "temperature");
        assert_eq!(order[0].direction, OrderDirection::Desc);
    }

    #[test]
    fn parse_order_by_multiple() {
        let p = params_from_query("orderBy=speed,!temperature");
        let order = p.parse_order_by();
        assert_eq!(order.len(), 2);
        assert_eq!(order[0].direction, OrderDirection::Asc);
        assert_eq!(order[1].direction, OrderDirection::Desc);
    }

    #[test]
    fn parse_order_by_semicolon_asc() {
        let p = params_from_query("orderBy=temperature;asc");
        let order = p.parse_order_by();
        assert_eq!(order.len(), 1);
        assert_eq!(order[0].attr, "temperature");
        assert_eq!(order[0].direction, OrderDirection::Asc);
    }

    #[test]
    fn parse_order_by_semicolon_desc() {
        let p = params_from_query("orderBy=temperature;desc");
        let order = p.parse_order_by();
        assert_eq!(order.len(), 1);
        assert_eq!(order[0].attr, "temperature");
        assert_eq!(order[0].direction, OrderDirection::Desc);
    }

    #[test]
    fn parse_order_by_dist_asc() {
        let p = params_from_query("orderBy=location;dist-asc");
        let order = p.parse_order_by();
        assert_eq!(order.len(), 1);
        assert_eq!(order[0].attr, "location");
        assert_eq!(order[0].direction, OrderDirection::DistAsc);
        assert!(order[0].direction.is_distance());
    }

    #[test]
    fn parse_order_by_dist_desc() {
        let p = params_from_query("orderBy=location;dist-desc");
        let order = p.parse_order_by();
        assert_eq!(order.len(), 1);
        assert_eq!(order[0].attr, "location");
        assert_eq!(order[0].direction, OrderDirection::DistDesc);
        assert!(order[0].direction.is_distance());
    }

    #[test]
    fn parse_order_by_mixed_directions() {
        let p = params_from_query("orderBy=name,age;desc,location;dist-asc");
        let order = p.parse_order_by();
        assert_eq!(order.len(), 3);
        assert_eq!(order[0].direction, OrderDirection::Asc);
        assert_eq!(order[1].direction, OrderDirection::Desc);
        assert_eq!(order[2].direction, OrderDirection::DistAsc);
    }

    #[test]
    fn parse_order_by_invalid_direction_skipped() {
        let p = params_from_query("orderBy=name;invalid");
        let order = p.parse_order_by();
        assert!(order.is_empty());
    }

    #[test]
    fn order_direction_sql_dir() {
        assert_eq!(OrderDirection::Asc.sql_dir(), "ASC");
        assert_eq!(OrderDirection::Desc.sql_dir(), "DESC");
        assert_eq!(OrderDirection::DistAsc.sql_dir(), "ASC");
        assert_eq!(OrderDirection::DistDesc.sql_dir(), "DESC");
    }

    #[test]
    fn parse_pick_omit() {
        let p = params_from_query("pick=id,type,speed&omit=scope");
        assert_eq!(p.parse_pick(), vec!["id", "type", "speed"]);
        assert_eq!(p.parse_omit(), vec!["scope"]);
    }

    #[test]
    fn effective_limit_defaults_to_20() {
        let p = params_from_query("");
        assert_eq!(p.effective_limit(), 20);
    }

    #[test]
    fn effective_limit_caps_at_1000() {
        let p = params_from_query("limit=5000");
        assert_eq!(p.effective_limit(), 1000);
    }

    #[test]
    fn effective_offset_defaults_to_0() {
        let p = params_from_query("");
        assert_eq!(p.effective_offset(), 0);
    }

    #[test]
    fn validate_rejects_invalid_id() {
        let p = params_from_query("id=thisisaninvaliduri");
        let err = p.validate().expect_err("expected invalid id error");
        assert!(matches!(err, QueryError::BadRequestData(_)));
        assert!(err.to_string().contains("Invalid id value"));
    }

    #[test]
    fn validate_rejects_empty_id_pattern() {
        let p = params_from_query("idPattern=%20");
        let err = p.validate().expect_err("expected invalid idPattern error");
        assert!(err.to_string().contains("Invalid idPattern: empty value"));
    }

    #[test]
    fn parse_order_from_accepted() {
        let p = params_from_query("orderFrom=%5B13.3986%2C52.5547%5D");
        assert_eq!(p.order_from.as_deref(), Some("[13.3986,52.5547]"));
        assert!(p.validate().is_ok());
    }

    #[test]
    fn parse_order_from_with_geometry() {
        let p = params_from_query("orderFrom=%5B%5B2.3%2C48.8%5D%2C%5B2.4%2C48.9%5D%5D&orderGeometry=LineString");
        assert_eq!(p.order_from.as_deref(), Some("[[2.3,48.8],[2.4,48.9]]"));
        assert_eq!(p.order_geometry.as_deref(), Some("LineString"));
        assert!(p.validate().is_ok());
    }

    #[test]
    fn validate_rejects_geometry_collection() {
        let p = params_from_query("orderFrom=%5B8%2C40%5D&orderGeometry=GeometryCollection");
        let err = p.validate().expect_err("expected GeometryCollection rejection");
        assert!(matches!(err, QueryError::BadRequestData(_)));
        assert!(err.to_string().contains("GeometryCollection"));
    }

    #[test]
    fn validate_rejects_invalid_order_geometry() {
        let p = params_from_query("orderFrom=%5B8%2C40%5D&orderGeometry=Hexagon");
        let err = p.validate().expect_err("expected invalid geometry rejection");
        assert!(matches!(err, QueryError::BadRequestData(_)));
        assert!(err.to_string().contains("Hexagon"));
    }

    #[test]
    fn validate_rejects_invalid_order_from_json() {
        let p = params_from_query("orderFrom=notjson");
        let err = p.validate().expect_err("expected invalid JSON rejection");
        assert!(matches!(err, QueryError::BadRequestData(_)));
        assert!(err.to_string().contains("orderFrom"));
    }
}
