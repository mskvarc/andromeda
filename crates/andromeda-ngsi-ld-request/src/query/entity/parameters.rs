//! Entity-query parameter definitions and their boundary tests.

use crate::{
    impl_shared_query_params,
    language_filter::deserialize_optional_language_filter,
    options::EntityRepresentationOptions,
    pagination::parameters::PageParameters,
    query::{
        SharedQueryParams,
        deserialization::{
            OptionalGeoQuery,
            deserialize_optional_attribute_names,
            deserialize_optional_dataset_selectors,
            deserialize_optional_filter_expr,
            deserialize_optional_iso8601_duration,
            deserialize_optional_pattern,
            deserialize_optional_projection_terms,
            deserialize_optional_uri_csv,
        },
        entity_selector::EntitySelectorParams,
    },
    representation_parameter::WrittenValue,
};
use andromeda_http_headers::language::LanguageFilter;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_query::{
    dataset::DatasetSelector,
    filter::expression::Expr,
    ordering::parameters::OrderingParameters,
    pattern::Pattern,
    projection::grammar::RawProjectionTerm,
    scope::query::ScopeQuery,
    type_selection::grammar::TypeExprRaw,
};
use andromeda_ngsi_ld_representation::{join_mode::JoinMode, response_format::ResponseFormat, sys_attrs::SysAttrsInclusion};
use iso8601_duration::Duration as Iso8601Duration;
use serde::Deserialize;
use std::num::NonZeroU32;

/// Raw HTTP query parameters for entity queries, deserialized from the request URL.
///
/// Corresponds to the query parameters defined in NGSI-LD 5.7.2 (Query Entities)
/// and related operations. All values are in their raw (compact) form before
/// context expansion.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntityQueryParameters {
    /// Entity identifiers to select, comma-separated (clause 6.3.x `id`).
    #[serde(default, deserialize_with = "deserialize_optional_uri_csv")]
    pub id: Option<Vec<Identifier>>,
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
    /// The page asked for (clauses 5.5.9 and 6.3.10 `limit`, `offset` and
    /// `cursor`), decided by the broker's pagination policy.
    #[serde(flatten)]
    pub page: PageParameters,
    /// Whether the response reports the total number of matches (`count`).
    pub count: Option<bool>,
    /// Typed representation and system-attribute options (`options`).
    pub options: Option<WrittenValue<EntityRepresentationOptions>>,
    /// Whether the operation is confined to this broker (clause 5.10 `local`).
    #[serde(rename = "local")]
    pub local: Option<bool>,
    /// Members the response is narrowed to (clause 4.21 `pick`).
    #[serde(default, deserialize_with = "deserialize_optional_projection_terms")]
    pub pick: Option<Vec<RawProjectionTerm>>,
    /// Members the response drops (clause 4.21 `omit`).
    #[serde(default, deserialize_with = "deserialize_optional_projection_terms")]
    pub omit: Option<Vec<RawProjectionTerm>>,
    /// Typed ordering semantics shared by GET and POST query bindings.
    #[serde(flatten)]
    pub ordering: OrderingParameters,
    /// The scope query expression (clause 4.18 `scopeQ`).
    #[serde(rename = "scopeQ")]
    pub scope_q: Option<ScopeQuery>,
    /// Clause-4.15 natural-language selection preferences.
    #[serde(default, deserialize_with = "deserialize_optional_language_filter")]
    pub lang: Option<LanguageFilter>,
    /// Dataset instances to select, comma-separated (`datasetId`).
    #[serde(rename = "datasetId")]
    #[serde(default, deserialize_with = "deserialize_optional_dataset_selectors")]
    pub dataset_id: Option<Vec<DatasetSelector>>,
    /// Attributes whose `q` values are expanded against the context (`expandValues`).
    #[serde(rename = "expandValues")]
    #[serde(default, deserialize_with = "deserialize_optional_attribute_names")]
    pub expand_values: Option<Vec<AttributeName>>,
    /// Attributes whose `q` paths address JSON object keys (`jsonKeys`).
    #[serde(rename = "jsonKeys")]
    #[serde(default, deserialize_with = "deserialize_optional_attribute_names")]
    pub json_keys: Option<Vec<AttributeName>>,
    /// The representation the response uses (clause 4.5.1 `format`).
    pub format: Option<WrittenValue<ResponseFormat>>,
    /// How linked entities reach the response (clause 4.5.23 `join`).
    pub join: Option<JoinMode>,
    /// How many links deep the traversal goes (`joinLevel`).
    #[serde(rename = "joinLevel")]
    pub join_level: Option<NonZeroU32>,
    /// Entity identifiers the result must be linked from (`containedBy`).
    #[serde(rename = "containedBy")]
    #[serde(default, deserialize_with = "deserialize_optional_uri_csv")]
    pub contained_by: Option<Vec<Identifier>>,
    /// Whether the query creates an entity map for stable pagination (`entityMap`).
    #[serde(rename = "entityMap")]
    pub entity_map: Option<bool>,
    /// How long a created entity map lives, as an ISO 8601 duration (`entityMapLifetime`).
    #[serde(rename = "entityMapLifetime")]
    #[serde(default, deserialize_with = "deserialize_optional_iso8601_duration")]
    pub entity_map_lifetime: Option<Iso8601Duration>,
    /// Whether attributes split across Context Sources are merged after forwarding (`splitEntities`).
    #[serde(rename = "splitEntities")]
    pub split_entities: Option<bool>,
    /// The `GeoProperty` a `GeoJSON` response uses as its geometry (clause 6.3.15 `geometryProperty`).
    #[serde(rename = "geometryProperty")]
    pub geometry_property: Option<AttributeName>,
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
    /// Whether a `drop`/`keep` purge removes every `datasetId` instance of the
    /// targeted attributes (the Andromeda `purge_dataset_scope` extension).
    ///
    /// Contradictory with `datasetId`, which names one instance: `deleteAll`
    /// means "ignore datasetId". Inert unless the extension is enabled, in which
    /// case it adds clause-5.6.13 `deleteAll` precision to the standard purge.
    #[serde(rename = "deleteAll")]
    pub delete_all: Option<bool>,
}

impl_shared_query_params!(EntityQueryParameters {
    id: id,
    attrs: attrs,
    pick: pick,
    omit: omit,
    dataset_id: dataset_id
});

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{error::ParameterError, params_from_query, query::entity::operation::EntityQueryOperation};
    use andromeda_ngsi_ld_model::query::body::Query;
    use andromeda_ngsi_ld_query::{ordering::term::OrderDirection, projection::exclusivity::ProjectionConflict};

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
        pretty_assertions::assert_eq!(ids.iter().map(Identifier::as_str).collect::<Vec<_>>(), vec!["urn:a", "urn:b", "urn:c"]);
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
        let order = &p.ordering.order_by;
        pretty_assertions::assert_eq!(order.len(), 1);
        pretty_assertions::assert_eq!(order[0].attr.attr.as_str(), "temperature");
        pretty_assertions::assert_eq!(order[0].direction, OrderDirection::Asc);
    }

    #[test]
    fn parse_order_by_desc_with_bang() {
        let p = params_from_query("orderBy=!temperature");
        let order = &p.ordering.order_by;
        pretty_assertions::assert_eq!(order.len(), 1);
        pretty_assertions::assert_eq!(order[0].attr.attr.as_str(), "temperature");
        pretty_assertions::assert_eq!(order[0].direction, OrderDirection::Desc);
    }

    #[test]
    fn parse_order_by_multiple() {
        let p = params_from_query("orderBy=speed,!temperature");
        let order = &p.ordering.order_by;
        pretty_assertions::assert_eq!(order.len(), 2);
        pretty_assertions::assert_eq!(order[0].direction, OrderDirection::Asc);
        pretty_assertions::assert_eq!(order[1].direction, OrderDirection::Desc);
    }

    #[test]
    fn parse_order_by_semicolon_asc() {
        let p = params_from_query("orderBy=temperature;asc");
        let order = &p.ordering.order_by;
        pretty_assertions::assert_eq!(order.len(), 1);
        pretty_assertions::assert_eq!(order[0].attr.attr.as_str(), "temperature");
        pretty_assertions::assert_eq!(order[0].direction, OrderDirection::Asc);
    }

    #[test]
    fn parse_order_by_semicolon_desc() {
        let p = params_from_query("orderBy=temperature;desc");
        let order = &p.ordering.order_by;
        pretty_assertions::assert_eq!(order.len(), 1);
        pretty_assertions::assert_eq!(order[0].attr.attr.as_str(), "temperature");
        pretty_assertions::assert_eq!(order[0].direction, OrderDirection::Desc);
    }

    #[test]
    fn parse_order_by_dist_asc() {
        let p = params_from_query("orderBy=location;dist-asc");
        let order = &p.ordering.order_by;
        pretty_assertions::assert_eq!(order.len(), 1);
        pretty_assertions::assert_eq!(order[0].attr.attr.as_str(), "location");
        pretty_assertions::assert_eq!(order[0].direction, OrderDirection::DistAsc);
        assert!(order[0].direction.is_distance());
    }

    #[test]
    fn parse_order_by_dist_desc() {
        let p = params_from_query("orderBy=location;dist-desc");
        let order = &p.ordering.order_by;
        pretty_assertions::assert_eq!(order.len(), 1);
        pretty_assertions::assert_eq!(order[0].attr.attr.as_str(), "location");
        pretty_assertions::assert_eq!(order[0].direction, OrderDirection::DistDesc);
        assert!(order[0].direction.is_distance());
    }

    #[test]
    fn parse_order_by_mixed_directions() {
        let p = params_from_query("orderBy=name,age;desc,location;dist-asc");
        let order = &p.ordering.order_by;
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
        // `options` its deprecated predecessor, so an explicit `format` wins over a
        // conflicting `options`. `format=normalized&options=keyValues` must
        // render normalized, not simplified.
        let p = params_from_query("format=normalized&options=keyValues");
        pretty_assertions::assert_eq!(p.parse_options().expect("a supported options value").render_mode, ResponseFormat::Normalized);
        let p = params_from_query("format=concise&options=keyValues");
        pretty_assertions::assert_eq!(p.parse_options().expect("a supported options value").render_mode, ResponseFormat::Concise);
    }

    #[test]
    fn parse_options_matches_whole_tokens_not_substrings() {
        // `options` carries whole comma-separated tokens; a value that merely
        // contains `keyValues` as a substring is not the keyValues option, and
        // clause 6.3.20 refuses it rather than reading a rendering out of it.
        let refused = params_from_query("type=Vehicle&options=mykeyValues")
            .validate(EntityQueryOperation::Query)
            .expect_err("a substring of a token is not that token");
        assert!(matches!(refused, ParameterError::OptionsInvalid(_)), "{refused}");
    }

    #[test]
    fn parse_pick_omit() {
        let p = params_from_query("pick=id,type,speed&omit=scope");
        pretty_assertions::assert_eq!(
            p.pick.as_deref().map(|terms| terms.iter().map(ToString::to_string).collect::<Vec<_>>()),
            Some(vec!["id".to_string(), "type".to_string(), "speed".to_string()])
        );
        pretty_assertions::assert_eq!(
            p.omit.as_deref().map(|terms| terms.iter().map(ToString::to_string).collect::<Vec<_>>()),
            Some(vec!["scope".to_string()])
        );
    }

    /// Clause 4.21: `observation{temperature,humidity}` is one raw term naming
    /// the `observation` relationship with two linked-entity sub-terms.
    #[test]
    fn pick_with_a_linked_entity_group_parses_to_one_term_with_two_children() {
        let p = params_from_query("pick=observation%7Btemperature%2Chumidity%7D");
        let terms = p.pick.as_deref().expect("pick present");
        pretty_assertions::assert_eq!(terms.len(), 1);
        pretty_assertions::assert_eq!(terms[0].name.as_str(), "observation");
        pretty_assertions::assert_eq!(
            terms[0].linked.iter().map(|term| term.name.as_str()).collect::<Vec<_>>(),
            vec!["temperature", "humidity"]
        );
        assert!(p.omit.is_none());
    }

    #[test]
    fn pick_and_omit_together_rejected() {
        let p = params_from_query("type=Vehicle&pick=speed&omit=scope");
        let err = p.validate(EntityQueryOperation::Query).expect_err("expected mutual exclusivity error");
        assert!(matches!(err, ParameterError::Projection(ProjectionConflict::PickWithOmit)));
        assert!(err.to_string().contains("mutually exclusive"));
    }

    #[test]
    fn pick_or_omit_with_attrs_is_rejected() {
        for query in ["type=Vehicle&pick=speed&attrs=fuelLevel", "type=Vehicle&omit=speed&attrs=fuelLevel"] {
            let err = params_from_query(query)
                .validate(EntityQueryOperation::Query)
                .expect_err("projection and attrs are mutually exclusive");
            assert!(matches!(err, ParameterError::Projection(ProjectionConflict::ProjectionWithAttrs)));
        }
    }

    /// Clause 5.7.2.4's ordering rule turns on whether a registered Context
    /// Source takes part, which the Context Registry answers and these
    /// parameters cannot, so validation passes an `orderBy` either way and
    /// the federation layer's ordering admission decides it.
    #[test]
    fn validate_leaves_the_ordering_rule_to_the_operation() {
        assert!(params_from_query("type=Vehicle&orderBy=speed").validate(EntityQueryOperation::Query).is_ok());
        assert!(
            params_from_query("type=Vehicle&orderBy=speed&local=true")
                .validate(EntityQueryOperation::Query)
                .is_ok()
        );
    }

    /// Clause 5.7.2.4: a query whose `pick` nests into a Linked Entity without
    /// asking for a join names members the response cannot carry.
    #[test]
    fn validate_rejects_a_nested_pick_without_a_join() {
        let p = params_from_query("type=Building&pick=linkedTo%7Bcategory%7D");
        let err = p
            .validate(EntityQueryOperation::Query)
            .expect_err("expected a projection/join consistency error");
        assert!(matches!(err, ParameterError::ProjectionLinkedTermsWithoutJoin), "got {err:?}");
    }

    /// Clause 5.7.2.4: the nesting may not reach past the traversal, and it
    /// becomes valid as soon as `joinLevel` covers it.
    #[test]
    fn validate_measures_the_nesting_against_the_join_level() {
        let too_deep = params_from_query("type=Building&join=flat&joinLevel=1&pick=a%7Bb%7Bc%7D%7D");
        let err = too_deep
            .validate(EntityQueryOperation::Query)
            .expect_err("expected a projection/join consistency error");
        assert!(
            matches!(err, ParameterError::ProjectionDepthExceedsJoinLevel { depth: 2, join_level: 1 }),
            "got {err:?}"
        );

        let deep_enough = params_from_query("type=Building&join=flat&joinLevel=2&pick=a%7Bb%7Bc%7D%7D");
        assert!(deep_enough.validate(EntityQueryOperation::Query).is_ok());
    }

    /// Clause 5.7.2.4: a `q` hop walks into a Linked Entity, and a query where
    /// "use of Linked Entity retrieval is not specified" asked for no such
    /// walk. Clause 4.5.23 makes `@none` the parameter's default, so stating it
    /// is the same request as omitting it.
    #[test]
    fn validate_rejects_a_traversing_filter_without_a_retrieving_join() {
        for query in ["type=Car&q=isParkedIn%7Bcategory%7D", "type=Car&join=%40none&q=isParkedIn%7Bcategory%7D"] {
            let err = params_from_query(query)
                .validate(EntityQueryOperation::Query)
                .expect_err("expected a filter/join consistency error");
            assert!(matches!(err, ParameterError::FilterTraversalWithoutJoin), "got {err:?} for {query}");
        }
    }

    /// Clause 5.7.2.4 "too deep query": the hop chain may not reach past the
    /// requested traversal, and becomes valid as soon as `joinLevel` covers it.
    /// `inline` is the mode a filter-only client wants — clause 4.5.23.2 has
    /// its response "only consist of Linking Entities".
    #[test]
    fn validate_measures_the_filter_hops_against_the_join_level() {
        let too_deep = params_from_query("type=Car&join=inline&joinLevel=1&q=isParkedIn%7BinDistrict%7Bzone%7D%7D");
        let err = too_deep
            .validate(EntityQueryOperation::Query)
            .expect_err("expected a filter/join consistency error");
        assert!(
            matches!(err, ParameterError::FilterDepthExceedsJoinLevel { depth: 2, join_level: 1 }),
            "got {err:?}"
        );

        let deep_enough = params_from_query("type=Car&join=inline&joinLevel=2&q=isParkedIn%7BinDistrict%7Bzone%7D%7D");
        assert!(deep_enough.validate(EntityQueryOperation::Query).is_ok());
    }

    /// Clause 5.6.21.4: Purge Entities retrieves no Linked Entity, so a hop
    /// and a nesting are refused whatever join the request names, while the
    /// same request is a valid query.
    #[test]
    fn validate_refuses_every_linked_reference_on_a_purge() {
        for query in [
            "type=Car&drop=speed&join=flat&q=isParkedIn%7Bcategory%7D",
            "type=Car&drop=speed&join=flat&pick=isParkedIn%7Bcategory%7D",
        ] {
            let err = params_from_query(query)
                .validate(EntityQueryOperation::Purge)
                .expect_err("purge retrieves nothing");
            assert!(matches!(err, ParameterError::LinkedRetrievalUnavailable { .. }), "got {err:?} for {query}");
            assert!(params_from_query(query).validate(EntityQueryOperation::Query).is_ok(), "{query}");
        }
    }

    /// The query reads its page beside its selection; the policy, not the
    /// parameter set, bounds it.
    #[test]
    fn the_page_is_read_beside_the_selection() {
        let p = params_from_query("type=Vehicle&limit=5000&offset=4");
        pretty_assertions::assert_eq!(p.page.limit, Some(5000));
        pretty_assertions::assert_eq!(p.page.offset, Some(4));
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
        let p = params_from_query("type=Vehicle&local=true&orderFrom=%5B13.3986%2C52.5547%5D");
        let geometry = p.ordering.geometry.as_ref().expect("typed ordering geometry");
        pretty_assertions::assert_eq!(
            serde_json::to_value(geometry.geometry.to_geojson()).expect("geometry serializes"),
            serde_json::json!({"type": "Point", "coordinates": [13.3986, 52.5547]})
        );
        assert!(p.validate(EntityQueryOperation::Query).is_ok());
    }

    #[test]
    fn parse_order_from_with_geometry() {
        let p = params_from_query("type=Vehicle&local=true&orderFrom=%5B%5B2.3%2C48.8%5D%2C%5B2.4%2C48.9%5D%5D&orderGeometry=LineString");
        let geometry = p.ordering.geometry.as_ref().expect("typed ordering geometry");
        pretty_assertions::assert_eq!(
            serde_json::to_value(geometry.geometry.to_geojson()).expect("geometry serializes"),
            serde_json::json!({"type": "LineString", "coordinates": [[2.3, 48.8], [2.4, 48.9]]})
        );
        assert!(p.validate(EntityQueryOperation::Query).is_ok());
    }

    #[test]
    fn validate_rejects_geometry_collection() {
        assert!(serde_urlencoded::from_str::<EntityQueryParameters>("orderFrom=%5B8%2C40%5D&orderGeometry=GeometryCollection").is_err());
    }

    #[test]
    fn validate_rejects_invalid_order_geometry() {
        assert!(serde_urlencoded::from_str::<EntityQueryParameters>("orderFrom=%5B8%2C40%5D&orderGeometry=Hexagon").is_err());
    }

    #[test]
    fn validate_rejects_invalid_order_from_json() {
        assert!(serde_urlencoded::from_str::<EntityQueryParameters>("orderFrom=notjson").is_err());
    }

    /// Clause 5.7.2.4 lists the type selector, the attribute list, the `q`
    /// filter, the geoquery and the local scope as the narrowing inputs, and
    /// lists no identifier among them: "it is not possible to retrieve a set of
    /// entities by only specifying desired Entity identifiers".
    #[test]
    fn each_listed_narrowing_input_satisfies_the_rule_and_an_identifier_does_not() {
        for query in [
            "type=Vehicle",
            "attrs=speed",
            "q=speed%3E50",
            "georel=near%3BmaxDistance==100&geometry=Point&coordinates=[1,2]",
            "local=true",
        ] {
            assert!(
                params_from_query(query).validate(EntityQueryOperation::Query).is_ok(),
                "{query} must narrow the query"
            );
        }
        for query in [
            "id=urn:ngsi-ld:T:1",
            "idPattern=urn:ngsi-ld:T:.*",
            "scopeQ=/Madrid",
            "id=urn:ngsi-ld:T:1&idPattern=urn:.*",
        ] {
            assert!(
                matches!(
                    params_from_query(query).validate(EntityQueryOperation::Query),
                    Err(ParameterError::SelectorRequired(_))
                ),
                "{query} must be too wide"
            );
        }
    }

    #[test]
    fn deserialization_rejects_an_empty_q_expression() {
        let parsed = serde_urlencoded::from_str::<EntityQueryParameters>("q=%20");
        assert!(parsed.is_err());
    }

    #[test]
    fn a_query_with_no_parameter_at_all_is_too_wide() {
        assert!(matches!(
            params_from_query("").validate(EntityQueryOperation::Query),
            Err(ParameterError::SelectorRequired(_))
        ));
    }

    #[test]
    fn a_wildcard_type_implies_local_scope_and_rejects_local_false() {
        assert!(params_from_query("type=*").is_local());
        assert!(params_from_query("type=*&local=true").is_local());
        assert!(!params_from_query("type=Vehicle").is_local());
        assert!(params_from_query("type=Vehicle&local=true").is_local());
        assert!(params_from_query("type=*").validate(EntityQueryOperation::Query).is_ok());
        assert!(matches!(
            params_from_query("type=*&local=false").validate(EntityQueryOperation::Query),
            Err(ParameterError::WildcardTypeWithLocalFalse)
        ));
    }

    #[test]
    fn canonical_query_preserves_selector_pairing_and_geo_query() {
        let request = serde_json::from_str::<Query>(
            r#"{"type":"Query","entities":[{"id":"urn:ngsi-ld:A","type":"T1"},{"id":"urn:ngsi-ld:B","type":"T2"}],"geoQ":{"georel":"near;maxDistance==2000","geometry":"Point","coordinates":[13.4,52.5],"geoproperty":"location"}}"#,
        )
        .expect("valid canonical query");
        let params = EntityQueryParameters::try_from(request).expect("applicable entity query");
        pretty_assertions::assert_eq!(params.entity_selectors.len(), 2);
        pretty_assertions::assert_eq!(params.entity_selectors[0].id.as_deref().map(|ids| ids[0].as_str()), Some("urn:ngsi-ld:A"));
        pretty_assertions::assert_eq!(params.entity_selectors[0].type_.as_ref().map(ToString::to_string).as_deref(), Some("T1"));
        pretty_assertions::assert_eq!(params.entity_selectors[1].id.as_deref().map(|ids| ids[0].as_str()), Some("urn:ngsi-ld:B"));
        pretty_assertions::assert_eq!(params.entity_selectors[1].type_.as_ref().map(ToString::to_string).as_deref(), Some("T2"));
        pretty_assertions::assert_eq!(
            params.geo.0.and_then(|geo| geo.geoproperty).as_ref().map(AsRef::<str>::as_ref),
            Some("location")
        );
    }

    #[test]
    fn canonical_query_rejects_malformed_per_selector_type_expressions() {
        assert!(serde_json::from_str::<Query>(r#"{"type":"Query","entities":[{"id":"urn:ngsi-ld:A","type":"(Vehicle"}]}"#).is_err());
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
    fn canonical_query_optional_fields_default_to_none() {
        let request = serde_json::from_str::<Query>(r#"{"type":"Query"}"#).expect("valid canonical query");
        let params = EntityQueryParameters::try_from(request).expect("applicable entity query");
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
        assert!(
            params_from_query("type=Vehicle&scopeQ=(/A;/B)&q=speed%3E50")
                .validate(EntityQueryOperation::Query)
                .is_ok()
        );
    }

    #[test]
    fn validate_rejects_drop_and_keep_together() {
        let p = params_from_query("type=Vehicle&drop=A&keep=B");
        let err = p.validate(EntityQueryOperation::Query).expect_err("expected mutual exclusivity error");
        assert!(matches!(err, ParameterError::DropKeepExclusive));
        assert!(err.to_string().contains("mutually exclusive"));
    }

    #[test]
    fn validate_accepts_drop_alone() {
        let p = params_from_query("type=Vehicle&drop=speed");
        assert!(p.validate(EntityQueryOperation::Query).is_ok());
    }

    #[test]
    fn validate_accepts_keep_alone() {
        let p = params_from_query("type=Vehicle&keep=speed");
        assert!(p.validate(EntityQueryOperation::Query).is_ok());
    }

    /// Clause 5.6.13: `deleteAll` and `datasetId` are contradictory, so the pair
    /// is a `BadRequestData`. The check is independent of whether the
    /// dataset-scope extension is enabled, which the handler gates separately.
    #[test]
    fn validate_rejects_dataset_id_with_delete_all() {
        let p = params_from_query("type=Vehicle&drop=speed&datasetId=urn:ngsi-ld:Dataset:1&deleteAll=true");
        let err = p.validate(EntityQueryOperation::Query).expect_err("datasetId and deleteAll contradict");
        assert!(matches!(err, ParameterError::DatasetIdWithDeleteAll), "got {err:?}");
        assert!(err.to_string().contains("mutually exclusive"));
    }

    /// `deleteAll` and `datasetId` each purge dataset-scoped instances on their
    /// own, so each validates alone.
    #[test]
    fn validate_accepts_delete_all_or_dataset_id_alone() {
        assert!(
            params_from_query("type=Vehicle&drop=speed&deleteAll=true")
                .validate(EntityQueryOperation::Query)
                .is_ok()
        );
        assert!(
            params_from_query("type=Vehicle&drop=speed&datasetId=urn:ngsi-ld:Dataset:1")
                .validate(EntityQueryOperation::Query)
                .is_ok()
        );
    }
}
