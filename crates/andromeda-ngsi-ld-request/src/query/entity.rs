//! Query parameters for the NGSI-LD Entity query API (5.7.2).

use crate::{
    body::query::QueryRequest,
    error::ParameterError,
    impl_shared_query_params,
    query::{
        deserialization::{
            OptionalGeoQuery,
            deserialize_optional_attribute_names,
            deserialize_optional_dataset_urns,
            deserialize_optional_filter_expr,
            deserialize_optional_pattern,
            deserialize_optional_uri_csv,
        },
        entity_selector::EntitySelectorParams,
        order_by::deserialize_optional_order_by,
    },
    representation_params::{deserialize_option_response_format, resolve_response_format, validate_representation_params},
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_query::{
    filter::expression::Expr,
    ordering::{collation::IcuCollation, term::OrderTerm},
    pattern::Pattern,
    scope::query::ScopeQuery,
    type_selection::grammar::TypeExprRaw,
};
use andromeda_ngsi_ld_representation::{join_mode::JoinMode, response_format::ResponseFormat, sys_attrs::SysAttrsInclusion};
use serde::{Deserialize, Deserializer};
use std::num::NonZeroU32;
use urn_rs::Urn;

/// Deserialise the `collation` query parameter, validating it as an ICU
/// collation identifier at the boundary (clause 4.23) rather than downstream.
///
/// # Errors
///
/// Returns the deserialiser's error when the value is present and is not a valid
/// ICU collation identifier.
fn deserialize_option_collation<'de, D>(deserializer: D) -> Result<Option<IcuCollation>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw: Option<String> = Option::deserialize(deserializer)?;
    raw.map(|value| IcuCollation::parse(&value).map_err(serde::de::Error::custom)).transpose()
}

/// Raw HTTP query parameters for entity queries, deserialized from the request URL.
///
/// Corresponds to the query parameters defined in NGSI-LD 6.3.5 (Query Entities)
/// and related operations. All values are in their raw (compact) form before
/// context expansion.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntityQueryParameters {
    /// Entity identifiers to select, comma-separated (clause 6.3.x `id`).
    #[serde(default, deserialize_with = "deserialize_optional_uri_csv")]
    pub id: Option<Vec<Uri>>,
    /// Entity types to select, as a type-selection expression (clause 4.17 `type`).
    #[serde(rename = "type")]
    pub entity_type: Option<TypeExprRaw>,
    /// A regular expression the selected identifiers must match (`idPattern`).
    #[serde(rename = "idPattern")]
    #[serde(default, deserialize_with = "deserialize_optional_pattern")]
    pub id_pattern: Option<Pattern>,
    /// Per-selector entity constraints from a POST query body's `entities[]`
    /// (clause 5.2.11), each still raw.
    ///
    /// Empty for a GET request, whose flat `id`/`type`/`idPattern` describe one
    /// implicit selector. When non-empty it is authoritative for compilation:
    /// the flat fields above are then a flattened summary that only the coarse
    /// `has_any_filter`/`validate` checks read, never the matcher. Never a query
    /// string parameter, so it is skipped by serde and populated by the POST
    /// body conversions.
    #[serde(skip)]
    pub entity_selectors: Vec<EntitySelectorParams>,
    /// Attributes the response is narrowed to, comma-separated (`attrs`).
    #[serde(default, deserialize_with = "deserialize_optional_attribute_names")]
    pub attrs: Option<Vec<AttributeName>>,
    /// The `q` filter expression (clause 4.9), parsed at the request boundary.
    #[serde(default, deserialize_with = "deserialize_optional_filter_expr")]
    pub q: Option<Expr>,
    /// Complete clause-4.10 geo-query parsed at URL extraction.
    #[serde(flatten)]
    pub geo: OptionalGeoQuery,
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
    #[serde(default, deserialize_with = "deserialize_optional_attribute_names")]
    pub pick: Option<Vec<AttributeName>>,
    /// Members the response drops (clause 6.3.15 `omit`).
    #[serde(default, deserialize_with = "deserialize_optional_attribute_names")]
    pub omit: Option<Vec<AttributeName>>,
    /// The ordering terms, comma-separated (clause 4.23 `orderBy`).
    #[serde(rename = "orderBy")]
    #[serde(default, deserialize_with = "deserialize_optional_order_by")]
    pub order_by: Option<Vec<OrderTerm>>,
    /// The scope query expression (clause 4.18 `scopeQ`).
    #[serde(rename = "scopeQ")]
    pub scope_q: Option<ScopeQuery>,
    /// Parsed by the language-negotiation extractor at the HTTP layer; retained
    /// here so `deny_unknown_fields` accepts `?lang=...` in the query string.
    #[allow(dead_code)]
    pub lang: Option<String>,
    /// Dataset instances to select, comma-separated (`datasetId`).
    #[serde(rename = "datasetId")]
    #[serde(default, deserialize_with = "deserialize_optional_dataset_urns")]
    pub dataset_id: Option<Vec<Urn>>,
    /// Attributes whose `q` values are expanded against the context (`expandValues`).
    #[serde(rename = "expandValues")]
    #[serde(default, deserialize_with = "deserialize_optional_attribute_names")]
    pub expand_values: Option<Vec<AttributeName>>,
    /// Attributes whose `q` paths address JSON object keys (`jsonKeys`).
    #[serde(rename = "jsonKeys")]
    #[serde(default, deserialize_with = "deserialize_optional_attribute_names")]
    pub json_keys: Option<Vec<AttributeName>>,
    /// The representation the response uses (clause 4.5.1 `format`).
    #[serde(default, deserialize_with = "deserialize_option_response_format")]
    pub format: Option<ResponseFormat>,
    /// How linked entities reach the response (clause 4.5.21 `join`).
    pub join: Option<JoinMode>,
    /// How many links deep the traversal goes (`joinLevel`).
    #[serde(rename = "joinLevel")]
    pub join_level: Option<NonZeroU32>,
    /// Entity identifiers the result must be linked from (`containedBy`).
    #[serde(rename = "containedBy")]
    #[serde(default, deserialize_with = "deserialize_optional_uri_csv")]
    pub contained_by: Option<Vec<Uri>>,
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
    pub geometry_property: Option<AttributeName>,
    /// ICU collation identifier for orderBy string sorting (S4.23).
    #[serde(default, deserialize_with = "deserialize_option_collation")]
    pub collation: Option<IcuCollation>,
    /// Reference coordinates for distance-based ordering (S5.2.43).
    #[serde(rename = "orderFrom")]
    pub order_from: Option<String>,
    /// Geometry type for orderFrom coordinates (S5.2.43). Defaults to "Point".
    #[serde(rename = "orderGeometry")]
    pub order_geometry: Option<String>,
    /// Context Source Filter expression (S6.4.3.2) for filtering which CSRs
    /// are contacted during distributed operations. Uses the q-expression grammar.
    #[serde(default, deserialize_with = "deserialize_optional_filter_expr")]
    pub csf: Option<Expr>,
    /// Attributes to remove from matching entities during purge (S5.6.21).
    /// Mutually exclusive with `keep`.
    #[serde(default, deserialize_with = "deserialize_optional_attribute_names")]
    pub drop: Option<Vec<AttributeName>>,
    /// Attributes to retain on matching entities during purge (S5.6.21).
    /// All other attributes are removed. Mutually exclusive with `drop`.
    #[serde(default, deserialize_with = "deserialize_optional_attribute_names")]
    pub keep: Option<Vec<AttributeName>>,
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
    /// Parses the `options` and `format` parameters into structured flags.
    ///
    /// Handles both the legacy `options` parameter (e.g. `"keyValues,sysAttrs"`)
    /// and the newer `format` parameter, with `format` taking precedence where
    /// both are provided (clause 4.5.1). The representation is resolved by the
    /// shared [`resolve_response_format`] so this and the single-entity GET path
    /// cannot drift apart.
    pub fn parse_options(&self) -> EntityQueryParametersOptions {
        let opts = self.options.as_deref().unwrap_or("");
        EntityQueryParametersOptions {
            render_mode: resolve_response_format(self.format, self.options.as_deref()),
            sys_attrs: if opts.split(',').map(str::trim).any(|token| token == "sysAttrs") {
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
        self.entity_type.is_some()
            || self.id.is_some()
            || self.id_pattern.is_some()
            || self.attrs.is_some()
            || self.geo.0.is_some()
            || self.scope_q.is_some()
            || self.q.is_some()
    }

    /// Validate query parameters.
    pub fn validate(&self) -> Result<(), ParameterError> {
        // Representation parameters (`format`/`options`), clause 4.5.1.
        validate_representation_params(self.format, self.options.as_deref())?;

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
        let mut id_pattern = None;
        let mut entity_selectors = Vec::new();

        if let Some(entities) = &request.entities {
            for selection in entities {
                if let Some(id) = selection.id() {
                    ids.push(id.clone());
                }
                if let Some(p) = selection.id_pattern() {
                    id_pattern = Some(p.clone());
                }
                // The precise per-selector grouping (clause 5.2.11), kept
                // alongside the flattened summary the fields above form.
                entity_selectors.push(EntitySelectorParams {
                    id: selection.id().map(|id| vec![id.clone()]),
                    id_pattern: selection.id_pattern().cloned(),
                    type_: selection.type_().cloned(),
                });
            }
        }

        EntityQueryParameters {
            id: if ids.is_empty() { None } else { Some(ids) },
            entity_type: None,
            id_pattern,
            entity_selectors,
            attrs: request.attrs,
            q: request.q,
            geo: OptionalGeoQuery(request.geo_q.map(|geo| geo.0)),
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

    fn rendered_type(p: &EntityQueryParameters) -> Option<String> {
        p.entity_type.as_ref().map(ToString::to_string)
    }

    #[test]
    fn deserialization_parses_comma_separated_ids() {
        let p = params_from_query("id=urn:a,urn:b,urn:c");
        let ids = p.id.expect("ids present");
        pretty_assertions::assert_eq!(ids.iter().map(Uri::as_str).collect::<Vec<_>>(), vec!["urn:a", "urn:b", "urn:c"]);
    }

    #[test]
    fn parse_types_splits_comma_separated() {
        let p = params_from_query("type=Vehicle,Building");
        pretty_assertions::assert_eq!(rendered_type(&p).as_deref(), Some("Vehicle,Building"));
    }

    #[test]
    fn parse_types_empty_when_absent() {
        let p = params_from_query("");
        assert!(rendered_type(&p).is_none());
    }

    #[test]
    fn parse_attrs_splits_comma_separated() {
        let p = params_from_query("attrs=speed,temperature");
        pretty_assertions::assert_eq!(
            p.attrs.as_deref().map(|names| names.iter().map(AttributeName::as_str).collect::<Vec<_>>()),
            Some(vec!["speed", "temperature"])
        );
    }

    #[test]
    fn parse_order_by_asc_default() {
        let p = params_from_query("orderBy=temperature");
        let order = p.order_by.as_deref().unwrap();
        pretty_assertions::assert_eq!(order.len(), 1);
        pretty_assertions::assert_eq!(order[0].attr, "temperature");
        pretty_assertions::assert_eq!(order[0].direction, OrderDirection::Asc);
    }

    #[test]
    fn parse_order_by_desc_with_bang() {
        let p = params_from_query("orderBy=!temperature");
        let order = p.order_by.as_deref().unwrap();
        pretty_assertions::assert_eq!(order.len(), 1);
        pretty_assertions::assert_eq!(order[0].attr, "temperature");
        pretty_assertions::assert_eq!(order[0].direction, OrderDirection::Desc);
    }

    #[test]
    fn parse_order_by_multiple() {
        let p = params_from_query("orderBy=speed,!temperature");
        let order = p.order_by.as_deref().unwrap();
        pretty_assertions::assert_eq!(order.len(), 2);
        pretty_assertions::assert_eq!(order[0].direction, OrderDirection::Asc);
        pretty_assertions::assert_eq!(order[1].direction, OrderDirection::Desc);
    }

    #[test]
    fn parse_order_by_semicolon_asc() {
        let p = params_from_query("orderBy=temperature;asc");
        let order = p.order_by.as_deref().unwrap();
        pretty_assertions::assert_eq!(order.len(), 1);
        pretty_assertions::assert_eq!(order[0].attr, "temperature");
        pretty_assertions::assert_eq!(order[0].direction, OrderDirection::Asc);
    }

    #[test]
    fn parse_order_by_semicolon_desc() {
        let p = params_from_query("orderBy=temperature;desc");
        let order = p.order_by.as_deref().unwrap();
        pretty_assertions::assert_eq!(order.len(), 1);
        pretty_assertions::assert_eq!(order[0].attr, "temperature");
        pretty_assertions::assert_eq!(order[0].direction, OrderDirection::Desc);
    }

    #[test]
    fn parse_order_by_dist_asc() {
        let p = params_from_query("orderBy=location;dist-asc");
        let order = p.order_by.as_deref().unwrap();
        pretty_assertions::assert_eq!(order.len(), 1);
        pretty_assertions::assert_eq!(order[0].attr, "location");
        pretty_assertions::assert_eq!(order[0].direction, OrderDirection::DistAsc);
        assert!(order[0].direction.is_distance());
    }

    #[test]
    fn parse_order_by_dist_desc() {
        let p = params_from_query("orderBy=location;dist-desc");
        let order = p.order_by.as_deref().unwrap();
        pretty_assertions::assert_eq!(order.len(), 1);
        pretty_assertions::assert_eq!(order[0].attr, "location");
        pretty_assertions::assert_eq!(order[0].direction, OrderDirection::DistDesc);
        assert!(order[0].direction.is_distance());
    }

    #[test]
    fn parse_order_by_mixed_directions() {
        let p = params_from_query("orderBy=name,age;desc,location;dist-asc");
        let order = p.order_by.as_deref().unwrap();
        pretty_assertions::assert_eq!(order.len(), 3);
        pretty_assertions::assert_eq!(order[0].direction, OrderDirection::Asc);
        pretty_assertions::assert_eq!(order[1].direction, OrderDirection::Desc);
        pretty_assertions::assert_eq!(order[2].direction, OrderDirection::DistAsc);
    }

    #[test]
    fn parse_order_by_rejects_unrecognized_direction() {
        // Clause 4.23 defines exactly asc/desc/dist-asc/dist-desc. An
        // unrecognised direction is a malformed request, not a term to silently
        // drop -- dropping it would quietly discard the client's ordering.
        let parsed = serde_urlencoded::from_str::<EntityQueryParameters>("orderBy=name;invalid");
        assert!(parsed.is_err());
    }

    #[test]
    fn parse_options_format_takes_precedence_over_options_for_render_mode() {
        // Clause 4.5.1: `format` is the current representation selector and
        // `options` its legacy predecessor, so an explicit `format` wins over a
        // conflicting `options`. `format=normalized&options=keyValues` must
        // render normalized, not simplified.
        let p = params_from_query("format=normalized&options=keyValues");
        pretty_assertions::assert_eq!(p.parse_options().render_mode, ResponseFormat::Normalized);
        let p = params_from_query("format=concise&options=keyValues");
        pretty_assertions::assert_eq!(p.parse_options().render_mode, ResponseFormat::Concise);
    }

    #[test]
    fn parse_options_matches_whole_tokens_not_substrings() {
        // `options` carries whole comma-separated tokens; a value that merely
        // contains `keyValues` as a substring is not the keyValues option.
        let p = params_from_query("options=mykeyValues");
        pretty_assertions::assert_eq!(p.parse_options().render_mode, ResponseFormat::Normalized);
    }

    #[test]
    fn parse_pick_omit() {
        let p = params_from_query("pick=id,type,speed&omit=scope");
        pretty_assertions::assert_eq!(
            p.pick.as_deref().map(|names| names.iter().map(AttributeName::as_str).collect::<Vec<_>>()),
            Some(vec!["id", "type", "speed"])
        );
        pretty_assertions::assert_eq!(
            p.omit.as_deref().map(|names| names.iter().map(AttributeName::as_str).collect::<Vec<_>>()),
            Some(vec!["scope"])
        );
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
    fn deserialization_rejects_invalid_id() {
        assert!(serde_urlencoded::from_str::<EntityQueryParameters>("id=thisisaninvaliduri").is_err());
    }

    #[test]
    fn deserialization_rejects_empty_id_pattern() {
        assert!(serde_urlencoded::from_str::<EntityQueryParameters>("idPattern=%20").is_err());
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
    fn deserialization_rejects_an_empty_q_expression() {
        let parsed = serde_urlencoded::from_str::<EntityQueryParameters>("q=%20");
        assert!(parsed.is_err());
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
                id: Some("urn:ngsi-ld:Vehicle:1".parse().expect("valid URI")),
                id_pattern: None,
                type_: Some("Vehicle".parse().unwrap()),
            }]),
            attrs: Some(vec!["speed".parse().unwrap()]),
            q: Some("speed>50".parse().unwrap()),
            geo_q: None,
            limit: Some(10),
            offset: Some(5),
            count: Some(true),
        };
        let params: EntityQueryParameters = request.into();
        pretty_assertions::assert_eq!(params.id.as_deref().map(|ids| ids[0].as_str()), Some("urn:ngsi-ld:Vehicle:1"));
        assert!(params.entity_type.is_none());
        pretty_assertions::assert_eq!(params.attrs.as_deref().map(|names| names[0].as_str()), Some("speed"));
        pretty_assertions::assert_eq!(params.q.as_ref().map(ToString::to_string).as_deref(), Some("speed>50"));
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
                EntitySelection::Id("urn:ngsi-ld:Vehicle:1".parse().expect("valid URI")),
                EntitySelection::Id("urn:ngsi-ld:Vehicle:2".parse().expect("valid URI")),
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
        assert!(ids.iter().any(|id| id.as_str() == "urn:ngsi-ld:Vehicle:1"));
        assert!(ids.iter().any(|id| id.as_str() == "urn:ngsi-ld:Vehicle:2"));
        // (flattened summary; the per-selector grouping is asserted below)
        pretty_assertions::assert_eq!(params.entity_selectors.len(), 2, "each selector is preserved, not flattened");
        pretty_assertions::assert_eq!(
            params.entity_selectors[0].id.as_deref().map(|ids| ids[0].as_str()),
            Some("urn:ngsi-ld:Vehicle:1")
        );
        pretty_assertions::assert_eq!(
            params.entity_selectors[1].id.as_deref().map(|ids| ids[0].as_str()),
            Some("urn:ngsi-ld:Vehicle:2")
        );
    }

    #[test]
    fn from_query_request_keeps_each_selectors_id_and_type_paired() {
        // Clause 5.2.11: `{id:A,type:T1}` and `{id:B,type:T2}` are a union, so
        // the per-selector pairing must survive into `entity_selectors` rather
        // than flatten into an `id=A,B`/`type=T1,T2` cross product.
        use crate::body::query::EntitySelection;

        let request = QueryRequest {
            type_: None,
            entities: Some(vec![
                EntitySelection::Object {
                    id: Some("urn:ngsi-ld:A".parse().expect("valid URI")),
                    id_pattern: None,
                    type_: Some("T1".parse().unwrap()),
                },
                EntitySelection::Object {
                    id: Some("urn:ngsi-ld:B".parse().expect("valid URI")),
                    id_pattern: None,
                    type_: Some("T2".parse().unwrap()),
                },
            ]),
            attrs: None,
            q: None,
            geo_q: None,
            limit: None,
            offset: None,
            count: None,
        };
        let params: EntityQueryParameters = request.into();
        pretty_assertions::assert_eq!(params.entity_selectors.len(), 2);
        pretty_assertions::assert_eq!(params.entity_selectors[0].id.as_deref().map(|ids| ids[0].as_str()), Some("urn:ngsi-ld:A"));
        pretty_assertions::assert_eq!(params.entity_selectors[0].type_.as_ref().map(ToString::to_string).as_deref(), Some("T1"));
        pretty_assertions::assert_eq!(params.entity_selectors[1].id.as_deref().map(|ids| ids[0].as_str()), Some("urn:ngsi-ld:B"));
        pretty_assertions::assert_eq!(params.entity_selectors[1].type_.as_ref().map(ToString::to_string).as_deref(), Some("T2"));
    }

    #[test]
    fn deserialization_rejects_malformed_per_selector_type_expressions() {
        let parsed = serde_json::from_str::<QueryRequest>(r#"{"type":"Query","entities":[{"id":"urn:ngsi-ld:A","type":"(Vehicle"}]}"#);
        assert!(parsed.is_err());
    }

    #[test]
    fn from_query_request_with_geo_q() {
        let request: QueryRequest =
            serde_json::from_str(r#"{"geoQ":{"georel":"near;maxDistance==2000","geometry":"Point","coordinates":[13.4,52.5],"geoproperty":"location"}}"#)
                .expect("valid body");
        let params: EntityQueryParameters = request.into();
        let geo = params.geo.0.expect("geo query");
        pretty_assertions::assert_eq!(geo.geoproperty.as_ref().map(AsRef::<str>::as_ref), Some("location"));
    }

    #[test]
    fn url_geo_query_is_decoded_as_one_typed_value() {
        let params: EntityQueryParameters =
            serde_urlencoded::from_str("georel=near%3BmaxDistance%3D%3D2000&geometry=Point&coordinates=%5B13.4%2C52.5%5D&geoproperty=location")
                .expect("valid geo query");
        let geo = params.geo.0.expect("decoded geo query");
        pretty_assertions::assert_eq!(geo.geoproperty.as_ref().map(AsRef::<str>::as_ref), Some("location"));
    }

    #[test]
    fn url_geo_query_rejects_an_incomplete_triple_at_extraction() {
        assert!(serde_urlencoded::from_str::<EntityQueryParameters>("georel=within&geometry=Point").is_err());
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
    fn validate_rejects_a_malformed_selection_expression_at_the_boundary() {
        // Clause 5.7.2: malformed language values are a 400 at the request
        // boundary, never a failure once the query reaches compilation.
        assert!(serde_urlencoded::from_str::<EntityQueryParameters>("type=(Building").is_err());
        assert!(serde_urlencoded::from_str::<EntityQueryParameters>("scopeQ=(/A;/B").is_err());
        assert!(params_from_query("type=Vehicle&scopeQ=/A;/B&q=speed%3E50").validate().is_ok());
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
