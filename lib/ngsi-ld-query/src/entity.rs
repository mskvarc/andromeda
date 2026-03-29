use crate::{error::QueryError, params::SharedQueryParams};
use serde::Deserialize;
use url::Url;
use urn::Urn;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
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
    /// ICU collation identifier for orderBy string sorting (§4.23).
    pub collation: Option<String>,
}

impl SharedQueryParams for EntityQueryParameters {
    fn raw_id(&self) -> Option<&str> {
        self.id.as_deref()
    }
    fn raw_entity_type(&self) -> Option<&str> {
        self.entity_type.as_deref()
    }
    fn raw_attrs(&self) -> Option<&str> {
        self.attrs.as_deref()
    }
    fn raw_pick(&self) -> Option<&str> {
        self.pick.as_deref()
    }
    fn raw_omit(&self) -> Option<&str> {
        self.omit.as_deref()
    }
    fn raw_dataset_id(&self) -> Option<&str> {
        self.dataset_id.as_deref()
    }
    fn raw_limit(&self) -> Option<u32> {
        self.limit
    }
    fn raw_offset(&self) -> Option<u32> {
        self.offset
    }
}

impl EntityQueryParameters {
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
                        // Spec §4.23.3: AttrName *1(;directionOp)
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

        if let Some(fmt) = &self.format {
            if !matches!(fmt.as_str(), "normalized" | "keyValues" | "simplified" | "concise") {
                return Err(QueryError::InvalidRequest(format!("Invalid format value: '{}'", fmt)));
            }
        }

        if let Some(ids) = &self.id {
            for id in ids.split(',').map(str::trim) {
                if id.is_empty() {
                    return Err(QueryError::InvalidRequest("Invalid id: empty value in comma-separated list".to_string()));
                }
                if !is_valid_entity_id(id) {
                    return Err(QueryError::BadRequestData(format!("Invalid id value: '{}'", id)));
                }
            }
        }

        if let Some(pattern) = &self.id_pattern {
            if pattern.trim().is_empty() {
                return Err(QueryError::BadRequestData("Invalid idPattern: empty value".to_string()));
            }
            regex::Regex::new(pattern).map_err(|e| QueryError::BadRequestData(format!("Invalid idPattern regex: {}", e)))?;
        }

        Ok(())
    }
}

fn is_valid_entity_id(id: &str) -> bool {
    if id.starts_with("urn:") {
        return id.parse::<Urn>().is_ok();
    }
    Url::parse(id).is_ok()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderTerm {
    pub attr: String,
    pub direction: OrderDirection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderDirection {
    Asc,
    Desc,
    /// Sort by distance in ascending order from the geo query reference geometry (§4.23).
    DistAsc,
    /// Sort by distance in descending order from the geo query reference geometry (§4.23).
    DistDesc,
}

impl OrderDirection {
    /// Whether this is a distance-based ordering direction.
    pub fn is_distance(&self) -> bool {
        matches!(self, OrderDirection::DistAsc | OrderDirection::DistDesc)
    }

    /// SQL direction keyword.
    pub fn sql_dir(&self) -> &'static str {
        match self {
            OrderDirection::Asc | OrderDirection::DistAsc => "ASC",
            OrderDirection::Desc | OrderDirection::DistDesc => "DESC",
        }
    }
}

#[derive(Debug, Default)]
#[allow(dead_code)]
pub struct EntityQueryParametersOptions {
    pub key_values: bool,
    pub sys_attrs: bool,
    pub count: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params_from_query(query: &str) -> EntityQueryParameters {
        serde_urlencoded::from_str(query).unwrap()
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
        let types = p.parse_types();
        assert_eq!(types, vec!["Vehicle", "Building"]);
    }

    #[test]
    fn parse_types_empty_when_absent() {
        let p = params_from_query("");
        assert!(p.parse_types().is_empty());
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
    fn validate_rejects_invalid_id_pattern() {
        let p = params_from_query("idPattern=(");
        let err = p.validate().expect_err("expected invalid idPattern error");
        assert!(err.to_string().contains("Invalid idPattern regex"));
    }
}
