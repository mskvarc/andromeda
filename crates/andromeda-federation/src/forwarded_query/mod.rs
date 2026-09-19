//! Per-block narrowing of a client's query parameters (clauses 4.3.6.1,
//! 4.3.6.3, 5.7.1.4, 5.7.2.4, 5.7.3.4 and 5.7.4.4).
//!
//! Clause 4.3.6.1 makes the broker responsible for respecting every constraint
//! a registration declares when it issues a distributed request: the forwarded
//! request names only the entity types, identifiers and attributes the
//! registration covers, because the source cannot tell that a registration
//! triggered the request and so cannot enforce those constraints itself. Clause
//! 4.3.6.3 makes each information block a self-contained coverage statement, so
//! the narrowing is per block: one registration produces one forwarded query
//! for each of its blocks that covers part of the request, never a single query
//! carrying the union of every block. The rest of the client's parameters
//! travel unchanged, except the filters that read attributes, which clauses
//! 5.7.2.4 and 5.7.4.4 remove under split entities so they can be applied once
//! to the aggregated result.

use crate::{
    config::SplitEntities,
    forwarded_parameter::ForwardedParameter,
    forwarded_query::{
        block_footprint::{AttrCoverage, BlockSelection, block_attrs, block_covers_id},
        client_query::ClientQuery,
    },
    request::SourcePage,
};
use andromeda_http_headers::language::LanguageFilter;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, temporal_format::TemporalFormat, uri::Uri};
use andromeda_ngsi_ld_model::registration::{ContextSourceRegistration, info::RegistrationInfo};
use andromeda_ngsi_ld_query::{dataset::DatasetSelector, geo::input::GeoQueryInput};
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use andromeda_ngsi_ld_request::{query::temporal::TemporalQueryParameters, representation_parameter::WrittenValue};
use chrono::{DateTime, SecondsFormat, Utc};
use std::num::NonZeroU32;
use tracing::warn;
use url::form_urlencoded;

pub mod block_footprint;
pub mod client_query;

/// The `options` token asking a source for its system-generated attributes
/// (clause 4.8; Table 6.3.11-1).
const SYS_ATTRS_OPTION: &str = "sysAttrs";
/// The maximum source page the Core API accepts before the hub applies its
/// own post-merge page window.
const SOURCE_PAGE_LIMIT: &str = "1000";

/// The query string one registration is sent: the client's parameters,
/// narrowed to what the registration covers and to what the split-entities
/// policy lets travel.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ForwardedQuery {
    /// The parameters in the order they are written.
    pairs: Vec<(ForwardedParameter, String)>,
}

impl ForwardedQuery {
    /// Narrow `client` to `registration`, one forwarded query per information
    /// block that covers part of the request; an empty result when no block
    /// covers any of the types, identifiers or attributes the client asked for,
    /// in which case the registration has nothing to answer.
    #[must_use]
    pub fn for_registration(client: ClientQuery<'_>, registration: &ContextSourceRegistration, split: SplitEntities) -> Vec<ForwardedQuery> {
        Self::for_registration_on_page(client, registration, split, None)
    }

    /// Narrow `client` to `registration` per information block, requesting
    /// `source_page` when a durable range consumer must walk the complete source
    /// result set. Identical block queries are collapsed so a source is
    /// contacted once for the same request.
    #[must_use]
    pub fn for_registration_on_page(
        client: ClientQuery<'_>,
        registration: &ContextSourceRegistration,
        split: SplitEntities,
        source_page: Option<SourcePage>,
    ) -> Vec<ForwardedQuery> {
        let mut queries: Vec<ForwardedQuery> = Vec::new();
        for block in &registration.coverage.information {
            if let Some(query) = Self::for_block(client, block, split, source_page)
                && !queries.contains(&query)
            {
                queries.push(query);
            }
        }
        queries
    }

    /// The forwarded query one information block produces, or `None` when the
    /// block covers none of the entities or attributes the client asked for
    /// (clause 4.3.6.3).
    fn for_block(client: ClientQuery<'_>, block: &RegistrationInfo, split: SplitEntities, source_page: Option<SourcePage>) -> Option<ForwardedQuery> {
        let mut query = ForwardedQuery::default();
        match client {
            ClientQuery::EntityQuery(params) => {
                query.push_selection(BlockSelection::of(
                    block,
                    params.id.as_deref(),
                    params.id_pattern.as_ref(),
                    params.entity_type.as_ref(),
                )?);
                query.push_source_page(source_page);
                match split {
                    SplitEntities::Complete => {
                        query.push_block_attrs(block_attrs(block, params.attrs.as_deref()))?;
                        query.push_optional(ForwardedParameter::Q, params.q.as_ref().map(ToString::to_string));
                        query.push_geo(params.geo.0.as_ref());
                        query.push_optional(ForwardedParameter::ScopeQ, params.scope_q.as_ref().map(ToString::to_string));
                    }
                    // Clause 5.7.2.4: the client's attribute filter is applied to
                    // the aggregated entities, never by a source holding a part.
                    // The block's declared attribute names are a registration
                    // constraint, not a client filter, so they still travel.
                    SplitEntities::Split => query.push_block_attrs(block_attrs(block, None))?,
                }
                query.push_optional(ForwardedParameter::Csf, params.csf.as_ref().map(ToString::to_string));
                query.push_dataset_ids(params.dataset_id.as_deref());
                query.push_lang(params.lang.as_ref());
                query.push_sys_attrs(params.options.as_ref().and_then(WrittenValue::if_supported).map(|options| options.sys_attrs));
            }
            ClientQuery::EntityRetrieve { id, params } => {
                // The entity is named by the caller; the block contributes only
                // its attribute constraint, and only when it covers the entity.
                if !block_covers_id(block, id) {
                    return None;
                }
                query.push_block_attrs(block_attrs(block, params.attrs.as_deref()))?;
                query.push_dataset_ids(params.dataset_id.as_deref());
                query.push_lang(params.lang.as_ref());
                query.push_sys_attrs(params.options.as_ref().and_then(WrittenValue::if_supported).map(|options| options.sys_attrs));
                query.push_traversal(params.join.map(|join| join.to_string()), params.join_level, params.contained_by.as_deref());
            }
            ClientQuery::Temporal(params) => {
                query.push_selection(BlockSelection::of(
                    block,
                    params.id.as_deref(),
                    params.id_pattern.as_ref(),
                    params.entity_type.as_ref(),
                )?);
                query.push_source_page(source_page);
                match split {
                    SplitEntities::Complete => {
                        query.push_block_attrs(block_attrs(block, params.attrs.as_deref()))?;
                        query.push_optional(ForwardedParameter::Q, params.q.as_ref().map(ToString::to_string));
                        query.push_geo(params.geo.0.as_ref());
                        query.push_optional(ForwardedParameter::ScopeQ, params.scope_q.as_ref().map(ToString::to_string));
                    }
                    // Clause 5.7.4.4: as for entities, the attribute-reading
                    // filters wait for the aggregation; the block's declared
                    // attribute names still travel as a registration constraint.
                    SplitEntities::Split => query.push_block_attrs(block_attrs(block, None))?,
                }
                query.push_optional(ForwardedParameter::Csf, params.csf.as_ref().map(ToString::to_string));
                query.push_dataset_ids(params.dataset_id.as_deref());
                query.push_lang(params.lang.as_ref());
                query.push_optional(ForwardedParameter::Timerel, params.timerel.map(|timerel| timerel.to_string()));
                query.push_optional(ForwardedParameter::TimeAt, params.time_at.map(rfc3339));
                query.push_optional(ForwardedParameter::EndTimeAt, params.end_time_at.map(rfc3339));
                query.push_optional(ForwardedParameter::Timeproperty, params.timeproperty.map(|property| property.to_string()));
                query.push_optional(ForwardedParameter::LastN, params.last_n.map(|last_n| last_n.to_string()));
                // Clause 6.3.12: `normalized` is the absence of a temporal
                // `format`, not a value a source accepts. The grouped extension
                // format is never forwarded (study §4.10): grouping is applied
                // by this broker after the merge, so a source is asked for the
                // plain aggregated representation and `aggrGroupBy` stays local.
                query.push_optional(
                    ForwardedParameter::Format,
                    params
                        .format
                        .as_ref()
                        .and_then(WrittenValue::if_supported)
                        .filter(|format| **format != TemporalFormat::Normalized)
                        .map(|format| match format {
                            TemporalFormat::GroupedValues => TemporalFormat::AggregatedValues.to_string(),
                            TemporalFormat::Normalized | TemporalFormat::TemporalValues | TemporalFormat::AggregatedValues => format.to_string(),
                        }),
                );
                query.push_temporal_options(params);
                query.push_optional(
                    ForwardedParameter::AggrMethods,
                    params
                        .aggr_methods
                        .as_deref()
                        .filter(|methods| !methods.is_empty())
                        .map(|methods| methods.iter().map(ToString::to_string).collect::<Vec<_>>().join(",")),
                );
                query.push_optional(
                    ForwardedParameter::AggrPeriodDuration,
                    params.aggr_period_duration.map(|period| period.as_iso8601()),
                );
                query.push_traversal(params.join.map(|join| join.to_string()), params.join_level, params.contained_by.as_deref());
            }
        }
        Some(query)
    }

    fn push_source_page(&mut self, source_page: Option<SourcePage>) {
        match source_page {
            Some(page) => {
                self.push(ForwardedParameter::Limit, page.limit.to_string());
                if page.offset > 0 {
                    self.push(ForwardedParameter::Offset, page.offset.to_string());
                }
            }
            None => self.push(ForwardedParameter::Limit, SOURCE_PAGE_LIMIT.to_owned()),
        }
    }

    /// The value of one parameter, when it is written.
    #[must_use]
    pub fn value_of(&self, parameter: ForwardedParameter) -> Option<&str> {
        self.pairs.iter().find(|(name, _)| *name == parameter).map(|(_, value)| value.as_str())
    }

    /// The `application/x-www-form-urlencoded` rendering the forwarded URL carries.
    #[must_use]
    pub fn to_query_string(&self) -> String {
        let mut serializer = form_urlencoded::Serializer::new(String::new());
        for (parameter, value) in &self.pairs {
            serializer.append_pair(&parameter.to_string(), value);
        }
        serializer.finish()
    }

    fn push(&mut self, parameter: ForwardedParameter, value: String) {
        self.pairs.push((parameter, value));
    }

    fn push_optional(&mut self, parameter: ForwardedParameter, value: Option<String>) {
        if let Some(value) = value {
            self.push(parameter, value);
        }
    }

    /// Write the entity selection one block contributes: the identifiers it
    /// narrows to or supplies, then the type, then the id pattern.
    fn push_selection(&mut self, selection: BlockSelection) {
        if !selection.ids.is_empty() {
            self.push(ForwardedParameter::Id, selection.ids.iter().map(Uri::as_str).collect::<Vec<_>>().join(","));
        }
        if let Some(entity_type) = selection.entity_type {
            self.push(ForwardedParameter::Type, entity_type.to_string());
        }
        self.push_optional(ForwardedParameter::IdPattern, selection.id_pattern);
    }

    /// Write the attribute list one block contributes, or signal with `None`
    /// that the block declares attribute names and covers none of the ones the
    /// client asked for, so the block has nothing to answer.
    fn push_block_attrs(&mut self, coverage: AttrCoverage) -> Option<()> {
        match coverage {
            AttrCoverage::Uncovered => None,
            AttrCoverage::Omit => Some(()),
            AttrCoverage::Write(names) => {
                self.push(ForwardedParameter::Attrs, names.iter().map(AttributeName::as_str).collect::<Vec<_>>().join(","));
                Some(())
            }
        }
    }

    /// Write the four clause 4.10 geo-query members.
    fn push_geo(&mut self, geo: Option<&GeoQueryInput>) {
        let Some(geo) = geo else {
            return;
        };
        let geometry = match serde_json::to_value(&geo.geometry) {
            Ok(geometry) => geometry,
            Err(error) => {
                warn!(%error, "a validated geo-query geometry could not be written back to its wire form");
                return;
            }
        };
        let (Some(geometry_type), Some(coordinates)) = (geometry.get("type").and_then(serde_json::Value::as_str), geometry.get("coordinates")) else {
            warn!("a validated geo-query geometry lost its type or coordinates on the way back to its wire form");
            return;
        };
        self.push(ForwardedParameter::Georel, geo.georel.to_string());
        self.push(ForwardedParameter::Geometry, geometry_type.to_string());
        self.push(ForwardedParameter::Coordinates, coordinates.to_string());
        self.push_optional(ForwardedParameter::Geoproperty, geo.geoproperty.as_ref().map(ToString::to_string));
    }

    fn push_dataset_ids(&mut self, selectors: Option<&[DatasetSelector]>) {
        if let Some(selectors) = selectors.filter(|selectors| !selectors.is_empty()) {
            self.push(
                ForwardedParameter::DatasetId,
                selectors.iter().map(ToString::to_string).collect::<Vec<_>>().join(","),
            );
        }
    }

    fn push_lang(&mut self, lang: Option<&LanguageFilter>) {
        if let Some(lang) = lang {
            self.push(
                ForwardedParameter::Lang,
                lang.preferences().iter().map(ToString::to_string).collect::<Vec<_>>().join(","),
            );
        }
    }

    /// Clause 4.3.6.1: every source supports the normalized representation and
    /// the broker renders the merged result itself, so the only `options` token
    /// worth forwarding is the request for system-generated attributes.
    fn push_sys_attrs(&mut self, sys_attrs: Option<SysAttrsInclusion>) {
        if sys_attrs == Some(SysAttrsInclusion::Include) {
            self.push(ForwardedParameter::Options, SYS_ATTRS_OPTION.to_string());
        }
    }

    /// The temporal `options` tokens: the representation a source answers in
    /// has to be the one the local history is rendered in for the two to merge,
    /// so the deprecated `options` spelling of the format travels as written.
    fn push_temporal_options(&mut self, params: &TemporalQueryParameters) {
        let Some(options) = params.options.as_ref().and_then(WrittenValue::if_supported) else {
            return;
        };
        let mut tokens = Vec::new();
        if let Some(format) = options.format {
            tokens.push(format.to_string());
        }
        if options.sys_attrs == SysAttrsInclusion::Include {
            tokens.push(SYS_ATTRS_OPTION.to_string());
        }
        if !tokens.is_empty() {
            self.push(ForwardedParameter::Options, tokens.join(","));
        }
    }

    /// The clause 4.5.23 traversal continuation of a Linked Entity retrieval.
    fn push_traversal(&mut self, join: Option<String>, join_level: Option<NonZeroU32>, contained_by: Option<&[Uri]>) {
        self.push_optional(ForwardedParameter::Join, join);
        self.push_optional(ForwardedParameter::JoinLevel, join_level.map(|level| level.to_string()));
        if let Some(held) = contained_by.filter(|held| !held.is_empty()) {
            self.push(ForwardedParameter::ContainedBy, held.iter().map(Uri::as_str).collect::<Vec<_>>().join(","));
        }
    }
}

/// An instant in the RFC 3339 form the temporal parameters are written in.
fn rfc3339(instant: DateTime<Utc>) -> String {
    instant.to_rfc3339_opts(SecondsFormat::AutoSi, true)
}

#[cfg(test)]
mod tests {
    use crate::{
        config::SplitEntities,
        forwarded_parameter::ForwardedParameter,
        forwarded_query::{ForwardedQuery, client_query::ClientQuery},
        request::SourcePage,
    };
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
    use andromeda_ngsi_ld_request::{
        entity::GetEntityParams,
        query::{entity::parameters::EntityQueryParameters, temporal::TemporalQueryParameters},
    };
    use serde_json::{Value, json};

    fn registration(information: &Value) -> ContextSourceRegistration {
        serde_json::from_value(json!({
            "id": "urn:ngsi-ld:csr:1",
            "type": "ContextSourceRegistration",
            "endpoint": "https://peer.example.org",
            "information": information,
            "status": "ok",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        }))
        .expect("valid registration")
    }

    fn entity_query(raw: &str) -> EntityQueryParameters {
        serde_urlencoded::from_str(raw).expect("valid entity query")
    }

    fn temporal_query(raw: &str) -> TemporalQueryParameters {
        serde_urlencoded::from_str(raw).expect("valid temporal query")
    }

    fn uri(raw: &str) -> Uri {
        raw.parse().expect("valid URI")
    }

    fn strings(queries: &[ForwardedQuery]) -> Vec<String> {
        queries.iter().map(ForwardedQuery::to_query_string).collect()
    }

    const VEHICLE: &str = "https://uri.etsi.org/ngsi-ld/default-context/Vehicle";
    const DEVICE: &str = "https://uri.etsi.org/ngsi-ld/default-context/Device";
    const SPEED: &str = "https://uri.etsi.org/ngsi-ld/default-context/speed";
    const FUEL: &str = "https://uri.etsi.org/ngsi-ld/default-context/fuel";
    const VEHICLE_X: &str = "urn:ngsi-ld:Vehicle:X";

    /// Clauses 4.3.6.1 and 4.3.6.3: an exclusive block names one entity and its
    /// attributes, so a client asking only for the type still has the block's id
    /// and attribute added to the forwarded request.
    #[test]
    fn an_exclusive_block_forwards_its_id_and_attribute_for_a_type_only_query() {
        let block = registration(&json!([{"entities": [{"id": VEHICLE_X, "type": VEHICLE}], "propertyNames": [SPEED]}]));
        let query = entity_query("type=Vehicle");
        let forwarded = ForwardedQuery::for_registration(ClientQuery::EntityQuery(&query), &block, SplitEntities::Complete);
        pretty_assertions::assert_eq!(
            strings(&forwarded),
            vec!["id=urn%3Angsi-ld%3AVehicle%3AX&type=Vehicle&limit=1000&attrs=speed".to_string()]
        );
    }

    /// Clause 4.3.6.3: two blocks each cover a different type and attribute, so
    /// each forwards its own query -- never one query carrying the union, which
    /// would ask a source for a cross-product it never registered.
    #[test]
    fn two_blocks_forward_two_queries_without_a_cross_product() {
        let reg = registration(&json!([
            {"entities": [{"type": VEHICLE}], "propertyNames": [SPEED]},
            {"entities": [{"type": DEVICE}], "propertyNames": [FUEL]}
        ]));
        let forwarded = ForwardedQuery::for_registration(ClientQuery::EntityQuery(&entity_query("")), &reg, SplitEntities::Complete);
        pretty_assertions::assert_eq!(
            strings(&forwarded),
            vec![
                "type=Vehicle&limit=1000&attrs=speed".to_string(),
                "type=Device&limit=1000&attrs=fuel".to_string()
            ]
        );
    }

    /// A block declaring an attribute the client did not ask for covers nothing
    /// requested, so it produces no forwarded query.
    #[test]
    fn a_block_whose_attributes_the_client_did_not_request_yields_no_query() {
        let block = registration(&json!([{"propertyNames": [SPEED]}]));
        let forwarded = ForwardedQuery::for_registration(ClientQuery::EntityQuery(&entity_query("attrs=fuel")), &block, SplitEntities::Complete);
        pretty_assertions::assert_eq!(strings(&forwarded), Vec::<String>::new());
    }

    /// A block covering none of the requested types produces no forwarded query.
    #[test]
    fn a_block_covering_none_of_the_requested_types_yields_no_query() {
        let block = registration(&json!([{"entities": [{"type": VEHICLE}]}]));
        let forwarded = ForwardedQuery::for_registration(ClientQuery::EntityQuery(&entity_query("type=Device")), &block, SplitEntities::Complete);
        pretty_assertions::assert_eq!(strings(&forwarded), Vec::<String>::new());
    }

    /// A block that declares no attribute names covers every attribute, so the
    /// client's attribute list travels unchanged.
    #[test]
    fn a_type_only_block_forwards_the_client_attributes_unchanged() {
        let block = registration(&json!([{"entities": [{"type": VEHICLE}]}]));
        let query = entity_query("type=Vehicle&attrs=speed,fuel");
        let forwarded = ForwardedQuery::for_registration(ClientQuery::EntityQuery(&query), &block, SplitEntities::Complete);
        pretty_assertions::assert_eq!(forwarded.len(), 1);
        pretty_assertions::assert_eq!(forwarded[0].value_of(ForwardedParameter::Type), Some("Vehicle"));
        pretty_assertions::assert_eq!(forwarded[0].value_of(ForwardedParameter::Attrs), Some("speed,fuel"));
    }

    /// A block constraining identifiers by pattern forwards that pattern when the
    /// client named no identifier of its own.
    #[test]
    fn a_block_constraining_ids_by_pattern_forwards_that_pattern() {
        let block = registration(&json!([{"entities": [{"type": VEHICLE, "idPattern": "urn:ngsi-ld:Vehicle:.*"}]}]));
        let forwarded = ForwardedQuery::for_registration(ClientQuery::EntityQuery(&entity_query("")), &block, SplitEntities::Complete);
        pretty_assertions::assert_eq!(forwarded.len(), 1);
        pretty_assertions::assert_eq!(forwarded[0].value_of(ForwardedParameter::IdPattern), Some("urn:ngsi-ld:Vehicle:.*"));
        pretty_assertions::assert_eq!(forwarded[0].value_of(ForwardedParameter::Type), Some("Vehicle"));
    }

    /// Clause 5.7.2.4: under split entities the client's value, geo, scope and
    /// attribute filters are removed before forwarding, but the block's declared
    /// attribute names still travel as a registration constraint.
    #[test]
    fn split_entities_drop_the_client_filters_but_keep_the_block_attributes() {
        let block = registration(&json!([{"entities": [{"type": VEHICLE}], "propertyNames": [SPEED]}]));
        let query = entity_query(
            "type=Vehicle&q=speed%3E50&georel=near%3BmaxDistance%3D%3D1000&geometry=Point&coordinates=%5B8.0%2C44.0%5D&scopeQ=%2FMadrid&attrs=fuel&csf=mode%3D%3Dinclusive&datasetId=%40none",
        );
        let forwarded = ForwardedQuery::for_registration(ClientQuery::EntityQuery(&query), &block, SplitEntities::Split);
        pretty_assertions::assert_eq!(forwarded.len(), 1);
        let written = forwarded[0].to_query_string();
        for stripped in ["q=", "georel", "geometry", "coordinates", "scopeQ", "fuel"] {
            assert!(!written.contains(stripped), "{stripped} must be stripped under split entities: {written}");
        }
        pretty_assertions::assert_eq!(forwarded[0].value_of(ForwardedParameter::Type), Some("Vehicle"));
        pretty_assertions::assert_eq!(forwarded[0].value_of(ForwardedParameter::Attrs), Some("speed"));
        // The filter is written back in its canonical clause 4.9 text: a bare
        // token is a string literal, spelled as one.
        pretty_assertions::assert_eq!(forwarded[0].value_of(ForwardedParameter::Csf), Some("mode==\"inclusive\""));
        pretty_assertions::assert_eq!(forwarded[0].value_of(ForwardedParameter::DatasetId), Some("@none"));
    }

    /// Without split entities every filter is forwarded for remote application,
    /// the geo-query spelled back as its four clause 4.10 members.
    #[test]
    fn complete_entities_forward_the_value_geo_scope_and_source_filters() {
        let block = registration(&json!([{"entities": [{"type": VEHICLE}]}]));
        let query = entity_query(
            "type=Vehicle&q=speed%3E50&georel=near%3BmaxDistance%3D%3D1000&geometry=Point&coordinates=%5B8.0%2C44.0%5D&geoproperty=location&scopeQ=%2FMadrid&attrs=speed&csf=mode%3D%3Dinclusive&lang=en&options=sysAttrs",
        );
        let forwarded = ForwardedQuery::for_registration(ClientQuery::EntityQuery(&query), &block, SplitEntities::Complete);
        pretty_assertions::assert_eq!(forwarded.len(), 1);
        let forwarded = &forwarded[0];
        pretty_assertions::assert_eq!(forwarded.value_of(ForwardedParameter::Q), Some("speed>50"));
        pretty_assertions::assert_eq!(forwarded.value_of(ForwardedParameter::Georel), Some("near;maxDistance==1000"));
        pretty_assertions::assert_eq!(forwarded.value_of(ForwardedParameter::Geometry), Some("Point"));
        pretty_assertions::assert_eq!(forwarded.value_of(ForwardedParameter::Coordinates), Some("[8.0,44.0]"));
        pretty_assertions::assert_eq!(forwarded.value_of(ForwardedParameter::Geoproperty), Some("location"));
        pretty_assertions::assert_eq!(forwarded.value_of(ForwardedParameter::ScopeQ), Some("/Madrid"));
        pretty_assertions::assert_eq!(forwarded.value_of(ForwardedParameter::Attrs), Some("speed"));
        pretty_assertions::assert_eq!(forwarded.value_of(ForwardedParameter::Csf), Some("mode==\"inclusive\""));
        pretty_assertions::assert_eq!(forwarded.value_of(ForwardedParameter::Lang), Some("en"));
        pretty_assertions::assert_eq!(forwarded.value_of(ForwardedParameter::Options), Some("sysAttrs"));
    }

    /// Clause 4.3.6.1: sources answer in the normalized representation and the
    /// broker renders the merged result, so a representation choice is never
    /// forwarded; only the system-attribute request is.
    #[test]
    fn a_representation_choice_is_never_forwarded() {
        let block = registration(&json!([{"entities": [{"type": VEHICLE}]}]));
        let query = entity_query("type=Vehicle&format=keyValues&options=concise");
        let forwarded = ForwardedQuery::for_registration(ClientQuery::EntityQuery(&query), &block, SplitEntities::Complete);
        pretty_assertions::assert_eq!(strings(&forwarded), vec!["type=Vehicle&limit=1000".to_string()]);
    }

    #[test]
    fn an_explicit_source_page_replaces_the_post_merge_window() {
        let block = registration(&json!([{"entities": [{"type": VEHICLE}]}]));
        let query = entity_query("type=Vehicle&limit=20&offset=10");
        let forwarded = ForwardedQuery::for_registration_on_page(
            ClientQuery::EntityQuery(&query),
            &block,
            SplitEntities::Complete,
            Some(SourcePage { limit: 250, offset: 750 }),
        );
        pretty_assertions::assert_eq!(strings(&forwarded), vec!["type=Vehicle&limit=250&offset=750".to_string()]);
    }

    /// Two identical blocks produce the same forwarded query, which is issued
    /// once: a source is not contacted twice for the same request.
    #[test]
    fn identical_block_queries_are_deduplicated() {
        let reg = registration(&json!([{"entities": [{"type": VEHICLE}]}, {"entities": [{"type": VEHICLE}]}]));
        let forwarded = ForwardedQuery::for_registration(ClientQuery::EntityQuery(&entity_query("type=Vehicle")), &reg, SplitEntities::Complete);
        pretty_assertions::assert_eq!(strings(&forwarded), vec!["type=Vehicle&limit=1000".to_string()]);
    }

    /// Clause 5.7.4.4: a split temporal query keeps its temporal window, and a
    /// complete one carries the window, the instance limit and the aggregation
    /// members the local history is rendered with.
    #[test]
    fn a_temporal_query_keeps_its_window_under_split_and_everything_else_when_complete() {
        let block = registration(&json!([{"entities": [{"type": VEHICLE}]}]));
        let query = temporal_query(
            "type=Vehicle&timerel=between&timeAt=2024-01-01T00:00:00Z&endTimeAt=2024-02-01T00:00:00Z&timeproperty=modifiedAt&lastN=2&q=speed%3E50&scopeQ=%2FMadrid&attrs=speed&options=aggregatedValues,sysAttrs&aggrMethods=avg,max&aggrPeriodDuration=PT1H&csf=tier%3D%3D%22gold%22",
        );
        let split = ForwardedQuery::for_registration(ClientQuery::Temporal(&query), &block, SplitEntities::Split);
        pretty_assertions::assert_eq!(split.len(), 1);
        let split = &split[0];
        let written = split.to_query_string();
        assert!(
            !written.contains("q=") && !written.contains("scopeQ") && !written.contains("attrs="),
            "{written}"
        );
        pretty_assertions::assert_eq!(split.value_of(ForwardedParameter::Timerel), Some("between"));
        pretty_assertions::assert_eq!(split.value_of(ForwardedParameter::TimeAt), Some("2024-01-01T00:00:00Z"));
        pretty_assertions::assert_eq!(split.value_of(ForwardedParameter::EndTimeAt), Some("2024-02-01T00:00:00Z"));
        pretty_assertions::assert_eq!(split.value_of(ForwardedParameter::Timeproperty), Some("modifiedAt"));
        pretty_assertions::assert_eq!(split.value_of(ForwardedParameter::LastN), Some("2"));
        pretty_assertions::assert_eq!(split.value_of(ForwardedParameter::Csf), Some("tier==\"gold\""));

        let complete = ForwardedQuery::for_registration(ClientQuery::Temporal(&query), &block, SplitEntities::Complete);
        pretty_assertions::assert_eq!(complete.len(), 1);
        let complete = &complete[0];
        pretty_assertions::assert_eq!(complete.value_of(ForwardedParameter::Q), Some("speed>50"));
        pretty_assertions::assert_eq!(complete.value_of(ForwardedParameter::ScopeQ), Some("/Madrid"));
        pretty_assertions::assert_eq!(complete.value_of(ForwardedParameter::Attrs), Some("speed"));
        pretty_assertions::assert_eq!(complete.value_of(ForwardedParameter::Options), Some("aggregatedValues,sysAttrs"));
        pretty_assertions::assert_eq!(complete.value_of(ForwardedParameter::AggrMethods), Some("avg,max"));
        pretty_assertions::assert_eq!(complete.value_of(ForwardedParameter::AggrPeriodDuration), Some("PT1H"));
    }

    /// Clause 4.3.6.3: a per-block temporal query is narrowed the same way an
    /// entity query is -- the block's id and attribute are named even though the
    /// client asked only for the type.
    #[test]
    fn a_temporal_query_is_narrowed_per_block_like_an_entity_query() {
        let block = registration(&json!([{"entities": [{"id": VEHICLE_X, "type": VEHICLE}], "propertyNames": [SPEED]}]));
        let query = temporal_query("type=Vehicle&timerel=before&timeAt=2030-01-01T00:00:00Z");
        let forwarded = ForwardedQuery::for_registration(ClientQuery::Temporal(&query), &block, SplitEntities::Complete);
        pretty_assertions::assert_eq!(forwarded.len(), 1);
        pretty_assertions::assert_eq!(forwarded[0].value_of(ForwardedParameter::Id), Some(VEHICLE_X));
        pretty_assertions::assert_eq!(forwarded[0].value_of(ForwardedParameter::Type), Some("Vehicle"));
        pretty_assertions::assert_eq!(forwarded[0].value_of(ForwardedParameter::Attrs), Some("speed"));
        pretty_assertions::assert_eq!(forwarded[0].value_of(ForwardedParameter::Timerel), Some("before"));
    }

    /// Clause 4.5.23: a forwarded Linked Entity retrieval carries the traversal
    /// continuation, and the block declaring an attribute adds it even when the
    /// client named none.
    #[test]
    fn a_retrieval_forwards_its_traversal_continuation_and_the_block_attribute() {
        let block = registration(&json!([{"entities": [{"id": VEHICLE_X, "type": VEHICLE}], "propertyNames": [SPEED]}]));
        let id = uri(VEHICLE_X);
        let params: GetEntityParams =
            serde_urlencoded::from_str("join=flat&joinLevel=2&containedBy=urn:ngsi-ld:Building:1,urn:ngsi-ld:Building:2&datasetId=urn:ngsi-ld:ds:1")
                .expect("valid retrieve parameters");
        let forwarded = ForwardedQuery::for_registration(ClientQuery::EntityRetrieve { id: &id, params: &params }, &block, SplitEntities::Complete);
        pretty_assertions::assert_eq!(
            strings(&forwarded),
            vec![
                "attrs=speed&datasetId=urn%3Angsi-ld%3Ads%3A1&join=flat&joinLevel=2&containedBy=urn%3Angsi-ld%3ABuilding%3A1%2Curn%3Angsi-ld%3ABuilding%3A2"
                    .to_string()
            ]
        );
    }

    /// A retrieval whose target the block does not cover produces no forwarded
    /// query for that block.
    #[test]
    fn a_retrieval_skips_a_block_that_does_not_cover_the_entity() {
        let block = registration(&json!([{"entities": [{"id": "urn:ngsi-ld:Vehicle:Other", "type": VEHICLE}], "propertyNames": [SPEED]}]));
        let id = uri(VEHICLE_X);
        let params = GetEntityParams::default();
        let forwarded = ForwardedQuery::for_registration(ClientQuery::EntityRetrieve { id: &id, params: &params }, &block, SplitEntities::Complete);
        pretty_assertions::assert_eq!(strings(&forwarded), Vec::<String>::new());
    }

    /// The wildcard and a conjunction cannot be narrowed member by member, so
    /// they are forwarded whole; an alternation is narrowed per block, so each
    /// covering block forwards its own covered name.
    #[test]
    fn only_a_flat_alternation_of_types_is_narrowed_per_block() {
        let reg = registration(&json!([{"entities": [{"type": VEHICLE}]}, {"entities": [{"type": "https://example.org/Car"}]}]));
        let conjunction = entity_query("type=Vehicle%3BDevice");
        let forwarded = ForwardedQuery::for_registration(ClientQuery::EntityQuery(&conjunction), &reg, SplitEntities::Complete);
        // Both blocks forward the conjunction whole, so the two queries are
        // identical and collapse to one.
        pretty_assertions::assert_eq!(forwarded.len(), 1);
        pretty_assertions::assert_eq!(forwarded[0].value_of(ForwardedParameter::Type), Some("(Vehicle;Device)"));

        let alternation = entity_query("type=Vehicle,Car,Device");
        let forwarded = ForwardedQuery::for_registration(ClientQuery::EntityQuery(&alternation), &reg, SplitEntities::Complete);
        let types = forwarded
            .iter()
            .filter_map(|query| query.value_of(ForwardedParameter::Type))
            .collect::<Vec<_>>();
        pretty_assertions::assert_eq!(types, vec!["Vehicle", "Car"]);
    }
}
