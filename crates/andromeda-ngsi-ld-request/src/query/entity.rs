//! Query parameters for the NGSI-LD Entity query API (5.7.2).

use crate::{body::query::QueryRequest, error::ParameterError, impl_shared_query_params, query::is_valid_uri};
use andromeda_ngsi_ld_query::{
    error::QueryError,
    ordering::{
        collation::IcuCollation,
        term::{OrderDirection, OrderTerm},
    },
};
use andromeda_ngsi_ld_representation::{response_format::ResponseFormat, sys_attrs::SysAttrsInclusion};
use serde::Deserialize;

/// Raw HTTP query parameters for entity queries, deserialized from the request URL.
///
/// Corresponds to the query parameters defined in NGSI-LD 6.3.5 (Query Entities)
/// and related operations. All values are in their raw (compact) form before
/// context expansion.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntityQueryParameters {
    /// Entity identifiers to select, comma-separated (clause 6.3.x `id`).
    pub id: Option<String>,
    /// Entity types to select, as a type-selection expression (clause 4.17 `type`).
    #[serde(rename = "type")]
    pub entity_type: Option<String>,
    /// A regular expression the selected identifiers must match (`idPattern`).
    #[serde(rename = "idPattern")]
    pub id_pattern: Option<String>,
    /// Attributes the response is narrowed to, comma-separated (`attrs`).
    pub attrs: Option<String>,
    /// The `q` filter expression (clause 4.9), in compacted form.
    pub q: Option<String>,
    /// The spatial relationship of the geo-query (clause 4.10 `georel`).
    pub georel: Option<String>,
    /// The GeoJSON geometry type of the geo-query's reference (`geometry`).
    pub geometry: Option<String>,
    /// The coordinates of the geo-query's reference geometry (`coordinates`).
    pub coordinates: Option<String>,
    /// The GeoProperty the geo-query applies to (`geoproperty`).
    pub geoproperty: Option<String>,
    /// The page size (clause 5.5.9 `limit`).
    pub limit: Option<u32>,
    /// How many results to skip before the page (`offset`).
    pub offset: Option<u32>,
    /// Whether the response reports the total number of matches (`count`).
    pub count: Option<bool>,
    /// The legacy representation selector, comma-separated (`options`).
    pub options: Option<String>,
    /// Whether the operation is confined to this broker (clause 5.10 `local`).
    #[serde(rename = "local")]
    pub local: Option<bool>,
    /// Members the response is narrowed to (clause 6.3.15 `pick`).
    pub pick: Option<String>,
    /// Members the response drops (clause 6.3.15 `omit`).
    pub omit: Option<String>,
    /// The ordering terms, comma-separated (clause 4.23 `orderBy`).
    #[serde(rename = "orderBy")]
    pub order_by: Option<String>,
    /// The scope query expression (clause 4.18 `scopeQ`).
    #[serde(rename = "scopeQ")]
    pub scope_q: Option<String>,
    /// Parsed by the language-negotiation extractor at the HTTP layer; retained
    /// here so `deny_unknown_fields` accepts `?lang=...` in the query string.
    #[allow(dead_code)]
    pub lang: Option<String>,
    /// Dataset instances to select, comma-separated (`datasetId`).
    #[serde(rename = "datasetId")]
    pub dataset_id: Option<String>,
    /// Attributes whose `q` values are expanded against the context (`expandValues`).
    #[serde(rename = "expandValues")]
    pub expand_values: Option<String>,
    /// Attributes whose `q` paths address JSON object keys (`jsonKeys`).
    #[serde(rename = "jsonKeys")]
    pub json_keys: Option<String>,
    /// The representation the response uses (clause 4.5.1 `format`).
    pub format: Option<String>,
    /// How linked entities reach the response (clause 4.5.21 `join`).
    pub join: Option<String>,
    /// How many links deep the traversal goes (`joinLevel`).
    #[serde(rename = "joinLevel")]
    pub join_level: Option<u32>,
    /// Entity identifiers the result must be linked from (`containedBy`).
    #[serde(rename = "containedBy")]
    pub contained_by: Option<String>,
    /// Whether the query creates an entity map for stable pagination (`entityMap`).
    #[serde(rename = "entityMap")]
    pub entity_map: Option<bool>,
    /// How long a created entity map lives, as an ISO 8601 duration (`entityMapLifetime`).
    #[serde(rename = "entityMapLifetime")]
    pub entity_map_lifetime: Option<String>,
    /// Whether attributes split across Context Sources are merged after forwarding (`splitEntities`).
    #[serde(rename = "splitEntities")]
    pub split_entities: Option<bool>,
    /// The GeoProperty a GeoJSON response uses as its geometry (clause 6.3.8 `geometryProperty`).
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
    /// Context Source Filter expression (S6.4.3.2) for filtering which CSRs
    /// are contacted during distributed operations. Uses the q-expression grammar.
    pub csf: Option<String>,
    /// Attributes to remove from matching entities during purge (S5.6.21).
    /// Mutually exclusive with `keep`.
    pub drop: Option<String>,
    /// Attributes to retain on matching entities during purge (S5.6.21).
    /// All other attributes are removed. Mutually exclusive with `drop`.
    pub keep: Option<String>,
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
        let render_mode = if fmt == "keyValues" || fmt == "simplified" || opts.contains("keyValues") || opts.contains("simplified") {
            ResponseFormat::Simplified
        } else if fmt == "concise" || opts.contains("concise") {
            ResponseFormat::Concise
        } else {
            ResponseFormat::Normalized
        };
        EntityQueryParametersOptions {
            render_mode,
            sys_attrs: if opts.contains("sysAttrs") {
                SysAttrsInclusion::Include
            } else {
                SysAttrsInclusion::Strip
            },
            count: self.count.unwrap_or(false),
        }
    }

    /// Whether any entity filter parameter is present.
    ///
    /// Per NGSI-LD 5.7.2.4, at least one filter (type, id, idPattern, q, attrs,
    /// georel, scopeQ) must be provided unless `local=true`. A `q` parameter
    /// that is present but empty/whitespace-only does not count as a filter.
    pub fn has_any_filter(&self) -> bool {
        let q_is_empty = self.q.as_ref().is_some_and(|s| s.trim().is_empty());
        self.entity_type.is_some()
            || self.id.is_some()
            || self.id_pattern.is_some()
            || self.attrs.is_some()
            || self.georel.is_some()
            || self.scope_q.is_some()
            || (self.q.is_some() && !q_is_empty)
    }

    /// Validate query parameters.
    pub fn validate(&self) -> Result<(), ParameterError> {
        // Validate options parameter
        if let Some(opts) = &self.options {
            for opt in opts.split(',') {
                let opt = opt.trim();
                if !matches!(opt, "keyValues" | "simplified" | "sysAttrs" | "concise" | "normalized") {
                    return Err(ParameterError::OptionsInvalid(opt.to_string()));
                }
            }
        }

        if let Some(fmt) = &self.format
            && !matches!(fmt.as_str(), "normalized" | "keyValues" | "simplified" | "concise")
        {
            return Err(ParameterError::FormatInvalid(fmt.clone()));
        }

        if let Some(ids) = &self.id {
            for id in ids.split(',').map(str::trim) {
                if id.is_empty() {
                    return Err(ParameterError::Query(QueryError::ParamIdListEmpty));
                }
                if !is_valid_uri(id) {
                    return Err(ParameterError::Query(QueryError::ParamIdInvalid(id.to_string())));
                }
            }
        }

        if let Some(pattern) = &self.id_pattern
            && pattern.trim().is_empty()
        {
            return Err(ParameterError::IdPatternEmpty);
        }

        // Validate orderGeometry: must be a valid GeoJSON geometry type, not GeometryCollection (S5.2.43)
        if let Some(ref order_geometry) = self.order_geometry {
            match order_geometry.as_str() {
                "Point" | "MultiPoint" | "LineString" | "MultiLineString" | "Polygon" | "MultiPolygon" => {}
                "GeometryCollection" => {
                    return Err(ParameterError::OrderGeometryCollection);
                }
                other => {
                    return Err(ParameterError::OrderGeometryInvalid(other.to_string()));
                }
            }
        }

        // Validate orderFrom: must be valid JSON coordinates (S5.2.43)
        if let Some(ref order_from) = self.order_from {
            serde_json::from_str::<serde_json::Value>(order_from).map_err(|source| ParameterError::OrderFromInvalid {
                value: order_from.clone(),
                source,
            })?;
        }

        if let Some(ref collation) = self.collation {
            IcuCollation::parse(collation).map_err(|source| ParameterError::Query(QueryError::from(source)))?;
        }

        // drop and keep are mutually exclusive (S5.6.21)
        if self.drop.is_some() && self.keep.is_some() {
            return Err(ParameterError::DropKeepExclusive);
        }

        Ok(())
    }
}

/// Parsed response shaping options from the `options` and `format` parameters.
#[derive(Debug)]
pub struct EntityQueryParametersOptions {
    /// Output representation mode (normalized, concise, or simplified).
    pub render_mode: ResponseFormat,
    /// Whether to include system-generated attributes (`createdAt`, `modifiedAt`).
    pub sys_attrs: SysAttrsInclusion,
    /// When true, include a total count of matching entities in the response.
    pub count: bool,
}

impl From<QueryRequest> for EntityQueryParameters {
    fn from(request: QueryRequest) -> EntityQueryParameters {
        let mut ids = Vec::new();
        let mut types = Vec::new();
        let mut id_pattern = None;

        if let Some(entities) = request.entities {
            for selection in entities {
                if let Some(id) = selection.id() {
                    ids.push(id.to_string());
                }
                if let Some(t) = selection.type_() {
                    types.push(t.to_string());
                }
                if let Some(p) = selection.id_pattern() {
                    id_pattern = Some(p.to_string());
                }
            }
        }

        let (georel, geometry, coordinates, geoproperty) = if let Some(geo) = request.geo_q {
            (Some(geo.georel), Some(geo.geometry), Some(geo.coordinates.to_string()), geo.geoproperty)
        } else {
            (None, None, None, None)
        };

        EntityQueryParameters {
            id: if ids.is_empty() { None } else { Some(ids.join(",")) },
            entity_type: if types.is_empty() { None } else { Some(types.join(",")) },
            id_pattern,
            attrs: request.attrs.map(|a| a.join(",")),
            q: request.q,
            georel,
            geometry,
            coordinates,
            geoproperty,
            limit: request.limit,
            offset: request.offset,
            count: request.count,
            options: None,
            local: None,
            pick: None,
            omit: None,
            order_by: None,
            scope_q: None,
            lang: None,
            dataset_id: None,
            expand_values: None,
            json_keys: None,
            format: None,
            join: None,
            join_level: None,
            contained_by: None,
            entity_map: None,
            entity_map_lifetime: None,
            split_entities: None,
            geometry_property: None,
            collation: None,
            order_from: None,
            order_geometry: None,
            csf: None,
            drop: None,
            keep: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{params_from_query, query::SharedQueryParams};
    use andromeda_ngsi_ld_query::ordering::term::OrderDirection;

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
        pretty_assertions::assert_eq!(ids, vec!["urn:a", "urn:b", "urn:c"]);
    }

    #[test]
    fn parse_types_splits_comma_separated() {
        let p = params_from_query("type=Vehicle,Building");
        let types = parse_types(&p);
        pretty_assertions::assert_eq!(types, vec!["Vehicle", "Building"]);
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
        pretty_assertions::assert_eq!(attrs, vec!["speed", "temperature"]);
    }

    #[test]
    fn parse_order_by_asc_default() {
        let p = params_from_query("orderBy=temperature");
        let order = p.parse_order_by();
        pretty_assertions::assert_eq!(order.len(), 1);
        pretty_assertions::assert_eq!(order[0].attr, "temperature");
        pretty_assertions::assert_eq!(order[0].direction, OrderDirection::Asc);
    }

    #[test]
    fn parse_order_by_desc_with_bang() {
        let p = params_from_query("orderBy=!temperature");
        let order = p.parse_order_by();
        pretty_assertions::assert_eq!(order.len(), 1);
        pretty_assertions::assert_eq!(order[0].attr, "temperature");
        pretty_assertions::assert_eq!(order[0].direction, OrderDirection::Desc);
    }

    #[test]
    fn parse_order_by_multiple() {
        let p = params_from_query("orderBy=speed,!temperature");
        let order = p.parse_order_by();
        pretty_assertions::assert_eq!(order.len(), 2);
        pretty_assertions::assert_eq!(order[0].direction, OrderDirection::Asc);
        pretty_assertions::assert_eq!(order[1].direction, OrderDirection::Desc);
    }

    #[test]
    fn parse_order_by_semicolon_asc() {
        let p = params_from_query("orderBy=temperature;asc");
        let order = p.parse_order_by();
        pretty_assertions::assert_eq!(order.len(), 1);
        pretty_assertions::assert_eq!(order[0].attr, "temperature");
        pretty_assertions::assert_eq!(order[0].direction, OrderDirection::Asc);
    }

    #[test]
    fn parse_order_by_semicolon_desc() {
        let p = params_from_query("orderBy=temperature;desc");
        let order = p.parse_order_by();
        pretty_assertions::assert_eq!(order.len(), 1);
        pretty_assertions::assert_eq!(order[0].attr, "temperature");
        pretty_assertions::assert_eq!(order[0].direction, OrderDirection::Desc);
    }

    #[test]
    fn parse_order_by_dist_asc() {
        let p = params_from_query("orderBy=location;dist-asc");
        let order = p.parse_order_by();
        pretty_assertions::assert_eq!(order.len(), 1);
        pretty_assertions::assert_eq!(order[0].attr, "location");
        pretty_assertions::assert_eq!(order[0].direction, OrderDirection::DistAsc);
        assert!(order[0].direction.is_distance());
    }

    #[test]
    fn parse_order_by_dist_desc() {
        let p = params_from_query("orderBy=location;dist-desc");
        let order = p.parse_order_by();
        pretty_assertions::assert_eq!(order.len(), 1);
        pretty_assertions::assert_eq!(order[0].attr, "location");
        pretty_assertions::assert_eq!(order[0].direction, OrderDirection::DistDesc);
        assert!(order[0].direction.is_distance());
    }

    #[test]
    fn parse_order_by_mixed_directions() {
        let p = params_from_query("orderBy=name,age;desc,location;dist-asc");
        let order = p.parse_order_by();
        pretty_assertions::assert_eq!(order.len(), 3);
        pretty_assertions::assert_eq!(order[0].direction, OrderDirection::Asc);
        pretty_assertions::assert_eq!(order[1].direction, OrderDirection::Desc);
        pretty_assertions::assert_eq!(order[2].direction, OrderDirection::DistAsc);
    }

    #[test]
    fn parse_order_by_invalid_direction_skipped() {
        let p = params_from_query("orderBy=name;invalid");
        let order = p.parse_order_by();
        assert!(order.is_empty());
    }

    #[test]
    fn parse_pick_omit() {
        let p = params_from_query("pick=id,type,speed&omit=scope");
        pretty_assertions::assert_eq!(p.parse_pick(), vec!["id", "type", "speed"]);
        pretty_assertions::assert_eq!(p.parse_omit(), vec!["scope"]);
    }

    #[test]
    fn effective_limit_defaults_to_20() {
        let p = params_from_query("");
        pretty_assertions::assert_eq!(p.effective_limit(), 20);
    }

    #[test]
    fn effective_limit_caps_at_1000() {
        let p = params_from_query("limit=5000");
        pretty_assertions::assert_eq!(p.effective_limit(), 1000);
    }

    #[test]
    fn effective_offset_defaults_to_0() {
        let p = params_from_query("");
        pretty_assertions::assert_eq!(p.effective_offset(), 0);
    }

    #[test]
    fn validate_rejects_invalid_id() {
        let p = params_from_query("id=thisisaninvaliduri");
        let err = p.validate().expect_err("expected invalid id error");
        assert!(matches!(err, ParameterError::Query(QueryError::ParamIdInvalid(_))));
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
        pretty_assertions::assert_eq!(p.order_from.as_deref(), Some("[13.3986,52.5547]"));
        assert!(p.validate().is_ok());
    }

    #[test]
    fn parse_order_from_with_geometry() {
        let p = params_from_query("orderFrom=%5B%5B2.3%2C48.8%5D%2C%5B2.4%2C48.9%5D%5D&orderGeometry=LineString");
        pretty_assertions::assert_eq!(p.order_from.as_deref(), Some("[[2.3,48.8],[2.4,48.9]]"));
        pretty_assertions::assert_eq!(p.order_geometry.as_deref(), Some("LineString"));
        assert!(p.validate().is_ok());
    }

    #[test]
    fn validate_rejects_geometry_collection() {
        let p = params_from_query("orderFrom=%5B8%2C40%5D&orderGeometry=GeometryCollection");
        let err = p.validate().expect_err("expected GeometryCollection rejection");
        assert!(matches!(err, ParameterError::OrderGeometryCollection));
        assert!(err.to_string().contains("GeometryCollection"));
    }

    #[test]
    fn validate_rejects_invalid_order_geometry() {
        let p = params_from_query("orderFrom=%5B8%2C40%5D&orderGeometry=Hexagon");
        let err = p.validate().expect_err("expected invalid geometry rejection");
        assert!(matches!(err, ParameterError::OrderGeometryInvalid(_)));
        assert!(err.to_string().contains("Hexagon"));
    }

    #[test]
    fn validate_rejects_invalid_order_from_json() {
        let p = params_from_query("orderFrom=notjson");
        let err = p.validate().expect_err("expected invalid JSON rejection");
        assert!(matches!(err, ParameterError::OrderFromInvalid { .. }));
        assert!(err.to_string().contains("orderFrom"));
    }

    #[test]
    fn has_any_filter_with_type() {
        let p = params_from_query("type=Vehicle");
        assert!(p.has_any_filter());
    }

    #[test]
    fn has_any_filter_with_id() {
        let p = params_from_query("id=urn:ngsi-ld:T:1");
        assert!(p.has_any_filter());
    }

    #[test]
    fn has_any_filter_with_q() {
        let p = params_from_query("q=speed%3E50");
        assert!(p.has_any_filter());
    }

    #[test]
    fn has_any_filter_empty_q_does_not_count() {
        let p = params_from_query("q=%20");
        assert!(!p.has_any_filter());
    }

    #[test]
    fn has_any_filter_no_params() {
        let p = params_from_query("");
        assert!(!p.has_any_filter());
    }

    #[test]
    fn has_any_filter_local_only_does_not_count() {
        let p = params_from_query("local=true");
        assert!(!p.has_any_filter());
    }

    #[test]
    fn from_query_request_empty_entities() {
        let request = QueryRequest {
            type_: None,
            entities: None,
            attrs: None,
            q: None,
            geo_q: None,
            limit: None,
            offset: None,
            count: None,
        };
        let params: EntityQueryParameters = request.into();
        assert!(params.id.is_none());
        assert!(params.entity_type.is_none());
        assert!(params.id_pattern.is_none());
        assert!(params.attrs.is_none());
    }

    #[test]
    fn from_query_request_single_entity_with_id_and_type() {
        use crate::body::query::EntitySelection;

        let request = QueryRequest {
            type_: None,
            entities: Some(vec![EntitySelection::Object {
                id: Some("urn:ngsi-ld:Vehicle:1".to_string()),
                id_pattern: None,
                type_: Some("Vehicle".to_string()),
            }]),
            attrs: Some(vec!["speed".to_string()]),
            q: Some("speed>50".to_string()),
            geo_q: None,
            limit: Some(10),
            offset: Some(5),
            count: Some(true),
        };
        let params: EntityQueryParameters = request.into();
        pretty_assertions::assert_eq!(params.id.as_deref(), Some("urn:ngsi-ld:Vehicle:1"));
        pretty_assertions::assert_eq!(params.entity_type.as_deref(), Some("Vehicle"));
        pretty_assertions::assert_eq!(params.attrs.as_deref(), Some("speed"));
        pretty_assertions::assert_eq!(params.q.as_deref(), Some("speed>50"));
        pretty_assertions::assert_eq!(params.limit, Some(10));
        pretty_assertions::assert_eq!(params.offset, Some(5));
        pretty_assertions::assert_eq!(params.count, Some(true));
    }

    #[test]
    fn from_query_request_multiple_entities() {
        use crate::body::query::EntitySelection;

        let request = QueryRequest {
            type_: None,
            entities: Some(vec![
                EntitySelection::Id("urn:ngsi-ld:Vehicle:1".to_string()),
                EntitySelection::Id("urn:ngsi-ld:Vehicle:2".to_string()),
            ]),
            attrs: None,
            q: None,
            geo_q: None,
            limit: None,
            offset: None,
            count: None,
        };
        let params: EntityQueryParameters = request.into();
        let ids = params.id.expect("ids present");
        assert!(ids.contains("urn:ngsi-ld:Vehicle:1"));
        assert!(ids.contains("urn:ngsi-ld:Vehicle:2"));
    }

    #[test]
    fn from_query_request_with_geo_q() {
        use crate::body::query::GeoQueryRequest;

        let request = QueryRequest {
            type_: None,
            entities: None,
            attrs: None,
            q: None,
            geo_q: Some(GeoQueryRequest {
                georel: "near;maxDistance==2000".to_string(),
                geometry: "Point".to_string(),
                coordinates: serde_json::json!([13.4, 52.5]),
                geoproperty: Some("location".to_string()),
            }),
            limit: None,
            offset: None,
            count: None,
        };
        let params: EntityQueryParameters = request.into();
        pretty_assertions::assert_eq!(params.georel.as_deref(), Some("near;maxDistance==2000"));
        pretty_assertions::assert_eq!(params.geometry.as_deref(), Some("Point"));
        assert!(params.coordinates.is_some());
        pretty_assertions::assert_eq!(params.geoproperty.as_deref(), Some("location"));
    }

    #[test]
    fn from_query_request_optional_fields_default_to_none() {
        let request = QueryRequest {
            type_: None,
            entities: None,
            attrs: None,
            q: None,
            geo_q: None,
            limit: None,
            offset: None,
            count: None,
        };
        let params: EntityQueryParameters = request.into();
        assert!(params.options.is_none());
        assert!(params.local.is_none());
        assert!(params.format.is_none());
        assert!(params.join.is_none());
        assert!(params.join_level.is_none());
        assert!(params.scope_q.is_none());
        assert!(params.dataset_id.is_none());
    }

    #[test]
    fn validate_rejects_drop_and_keep_together() {
        let p = params_from_query("type=Vehicle&drop=A&keep=B");
        let err = p.validate().expect_err("expected mutual exclusivity error");
        assert!(matches!(err, ParameterError::DropKeepExclusive));
        assert!(err.to_string().contains("mutually exclusive"));
    }

    #[test]
    fn validate_accepts_drop_alone() {
        let p = params_from_query("type=Vehicle&drop=speed");
        assert!(p.validate().is_ok());
    }

    #[test]
    fn validate_accepts_keep_alone() {
        let p = params_from_query("type=Vehicle&keep=speed");
        assert!(p.validate().is_ok());
    }
}
