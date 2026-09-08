//! Per-registration narrowing of a client's query parameters (clauses 4.3.6.1,
//! 4.3.6.3, 5.7.1.4, 5.7.2.4, 5.7.3.4 and 5.7.4.4).
//!
//! Clause 4.3.6.1 makes the broker responsible for respecting every constraint
//! a registration declares when it issues a distributed request: the forwarded
//! request names only the entity types, identifiers and attributes the
//! registration covers, because the source cannot tell that a registration
//! triggered the request and so cannot enforce those constraints itself. The
//! rest of the client's parameters travel unchanged, except the filters that
//! read attributes, which clauses 5.7.2.4 and 5.7.4.4 remove under split
//! entities so they can be applied once to the aggregated result.

use crate::{
    config::SplitEntities,
    forwarded_parameter::ForwardedParameter,
    registration_selection::{compact_attribute_name, information_matches_entity},
    request::SourcePage,
};
use andromeda_http_headers::language::LanguageFilter;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, temporal_format::TemporalFormat, uri::Uri};
use andromeda_ngsi_ld_model::{entity_info::EntityTypeSelection, registration::ContextSourceRegistration};
use andromeda_ngsi_ld_query::{dataset::DatasetSelector, geo::input::GeoQueryInput, scope::query::ScopeQuery, type_selection::grammar::TypeExprRaw};
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use andromeda_ngsi_ld_request::{
    entity::GetEntityParams,
    query::{entity::parameters::EntityQueryParameters, temporal::TemporalQueryParameters},
};
use chrono::{DateTime, SecondsFormat, Utc};
use std::num::NonZeroU32;
use tracing::warn;
use url::form_urlencoded;

/// The `options` token asking a source for its system-generated attributes
/// (clause 4.8; Table 6.3.11-1).
const SYS_ATTRS_OPTION: &str = "sysAttrs";
/// The maximum source page the Core API accepts before the hub applies its
/// own post-merge page window.
const SOURCE_PAGE_LIMIT: &str = "1000";

/// The query a client sent, as the typed parameters of the operation it is.
#[derive(Debug, Clone, Copy)]
pub enum ClientQuery<'a> {
    /// A Query Entities request (clause 5.7.2).
    EntityQuery(&'a EntityQueryParameters),
    /// A Retrieve Entity request (clause 5.7.1).
    EntityRetrieve(&'a GetEntityParams),
    /// A temporal query or retrieval (clauses 5.7.3 and 5.7.4).
    Temporal(&'a TemporalQueryParameters),
}

impl ClientQuery<'_> {
    /// The clause 4.17 type selector the client wrote, if the operation has one.
    pub const fn entity_type(&self) -> Option<&TypeExprRaw> {
        match self {
            ClientQuery::EntityQuery(params) => params.entity_type.as_ref(),
            ClientQuery::Temporal(params) => params.entity_type.as_ref(),
            ClientQuery::EntityRetrieve(_) => None,
        }
    }

    /// The entity identifiers the client selected, if the operation has any.
    pub fn entity_ids(&self) -> Option<&[Uri]> {
        match self {
            ClientQuery::EntityQuery(params) => params.id.as_deref(),
            ClientQuery::Temporal(params) => params.id.as_deref(),
            ClientQuery::EntityRetrieve(_) => None,
        }
    }

    /// The scope query used to preselect registrations by their declared
    /// coverage footprint.
    pub const fn scope_q(&self) -> Option<&ScopeQuery> {
        match self {
            ClientQuery::EntityQuery(params) => params.scope_q.as_ref(),
            ClientQuery::Temporal(params) => params.scope_q.as_ref(),
            ClientQuery::EntityRetrieve(_) => None,
        }
    }
}

/// The query string one registration is sent: the client's parameters,
/// narrowed to what the registration covers and to what the split-entities
/// policy lets travel.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ForwardedQuery {
    /// The parameters in the order they are written.
    pairs: Vec<(ForwardedParameter, String)>,
}

impl ForwardedQuery {
    /// Narrow `client` to `registration`, or `None` when the registration
    /// covers none of the types, identifiers or attributes the client asked
    /// for, in which case it has nothing to answer and is not contacted.
    pub fn for_registration(client: ClientQuery<'_>, registration: &ContextSourceRegistration, split: SplitEntities) -> Option<ForwardedQuery> {
        Self::for_registration_on_page(client, registration, split, None)
    }

    /// Narrow `client` to `registration`, requesting `source_page` when a
    /// durable range consumer must walk the complete source result set.
    pub fn for_registration_on_page(
        client: ClientQuery<'_>,
        registration: &ContextSourceRegistration,
        split: SplitEntities,
        source_page: Option<SourcePage>,
    ) -> Option<ForwardedQuery> {
        let mut query = ForwardedQuery::default();
        match client {
            ClientQuery::EntityQuery(params) => {
                query.push_selection(params.id.as_deref(), params.entity_type.as_ref(), registration)?;
                query.push_optional(ForwardedParameter::IdPattern, params.id_pattern.as_ref().map(ToString::to_string));
                query.push_source_page(source_page);
                match split {
                    SplitEntities::Complete => {
                        query.push_attrs(params.attrs.as_deref(), registration)?;
                        query.push_optional(ForwardedParameter::Q, params.q.as_ref().map(ToString::to_string));
                        query.push_geo(params.geo.0.as_ref());
                        query.push_optional(ForwardedParameter::ScopeQ, params.scope_q.as_ref().map(ToString::to_string));
                    }
                    // Clause 5.7.2.4: the filters reading attributes are applied
                    // to the aggregated entities, never by a source holding a part.
                    SplitEntities::Split => {}
                }
                query.push_optional(ForwardedParameter::Csf, params.csf.as_ref().map(ToString::to_string));
                query.push_dataset_ids(params.dataset_id.as_deref());
                query.push_lang(params.lang.as_ref());
                query.push_sys_attrs(params.options.map(|options| options.sys_attrs));
            }
            ClientQuery::EntityRetrieve(params) => {
                query.push_attrs(params.attrs.as_deref(), registration)?;
                query.push_dataset_ids(params.dataset_id.as_deref());
                query.push_lang(params.lang.as_ref());
                query.push_sys_attrs(params.options.map(|options| options.sys_attrs));
                query.push_traversal(params.join.map(|join| join.to_string()), params.join_level, params.contained_by.as_deref());
            }
            ClientQuery::Temporal(params) => {
                query.push_selection(params.id.as_deref(), params.entity_type.as_ref(), registration)?;
                query.push_optional(ForwardedParameter::IdPattern, params.id_pattern.as_ref().map(ToString::to_string));
                query.push_source_page(source_page);
                match split {
                    SplitEntities::Complete => {
                        query.push_attrs(params.attrs.as_deref(), registration)?;
                        query.push_optional(ForwardedParameter::Q, params.q.as_ref().map(ToString::to_string));
                        query.push_geo(params.geo.0.as_ref());
                        query.push_optional(ForwardedParameter::ScopeQ, params.scope_q.as_ref().map(ToString::to_string));
                    }
                    // Clause 5.7.4.4: as for entities, the attribute-reading
                    // filters wait for the aggregation; the temporal query itself
                    // is not a filter on attributes and always travels.
                    SplitEntities::Split => {}
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
                // `format`, not a value a source accepts.
                query.push_optional(
                    ForwardedParameter::Format,
                    params
                        .format
                        .filter(|format| *format != TemporalFormat::Normalized)
                        .map(|format| format.to_string()),
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
    pub fn value_of(&self, parameter: ForwardedParameter) -> Option<&str> {
        self.pairs.iter().find(|(name, _)| *name == parameter).map(|(_, value)| value.as_str())
    }

    /// The `application/x-www-form-urlencoded` rendering the forwarded URL carries.
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

    /// Write the entity selection the registration covers; `None` when it
    /// covers none of the requested identifiers or types.
    fn push_selection(&mut self, ids: Option<&[Uri]>, entity_type: Option<&TypeExprRaw>, registration: &ContextSourceRegistration) -> Option<()> {
        if let Some(ids) = ids.filter(|ids| !ids.is_empty()) {
            let covered = covered_ids(ids, registration)?;
            self.push(ForwardedParameter::Id, covered.iter().map(Uri::as_str).collect::<Vec<_>>().join(","));
        }
        if let Some(entity_type) = entity_type {
            let covered = covered_type(entity_type, registration)?;
            self.push(ForwardedParameter::Type, covered.to_string());
        }
        Some(())
    }

    /// Write the attribute list the registration covers; `None` when the
    /// client asked for attributes and the registration declares none of them.
    fn push_attrs(&mut self, attrs: Option<&[AttributeName]>, registration: &ContextSourceRegistration) -> Option<()> {
        if let Some(attrs) = attrs.filter(|attrs| !attrs.is_empty()) {
            let covered = covered_attrs(attrs, registration)?;
            self.push(
                ForwardedParameter::Attrs,
                covered.iter().map(AttributeName::as_str).collect::<Vec<_>>().join(","),
            );
        }
        Some(())
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
        let Some(options) = params.options else {
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

/// The identifiers among `ids` that some information entry of the registration
/// covers, or `None` when it covers none of them.
fn covered_ids(ids: &[Uri], registration: &ContextSourceRegistration) -> Option<Vec<Uri>> {
    let covered = ids
        .iter()
        .filter(|id| {
            registration
                .coverage
                .information
                .iter()
                .any(|information| information_matches_entity(information, None, Some(id)))
        })
        .cloned() // clone: the forwarded query owns the identifiers it writes
        .collect::<Vec<_>>();
    (!covered.is_empty()).then_some(covered)
}

/// The part of the client's type selector the registration covers, or `None`
/// when it covers none of it.
///
/// Only a flat alternation of names can be narrowed; the wildcard and a
/// conjunction are forwarded whole, because removing a member would change
/// what they select rather than restrict it.
fn covered_type(entity_type: &TypeExprRaw, registration: &ContextSourceRegistration) -> Option<TypeExprRaw> {
    let selectors = registration
        .coverage
        .information
        .iter()
        .filter_map(|information| information.entities.as_deref())
        .flatten()
        .map(|selector| &selector.type_)
        .collect::<Vec<_>>();
    let unrestricted = registration.coverage.information.iter().any(|information| information.entities.is_none());
    if unrestricted || selectors.is_empty() {
        return Some(entity_type.clone()); // clone: the forwarded query owns the selector it writes
    }
    let leaves = match entity_type {
        TypeExprRaw::Type(name) => vec![name],
        TypeExprRaw::Or(children) => children
            .iter()
            .map(|child| match child {
                TypeExprRaw::Type(name) => Some(name),
                TypeExprRaw::Wildcard | TypeExprRaw::And(_) | TypeExprRaw::Or(_) => None,
            })
            .collect::<Option<Vec<_>>>()?,
        TypeExprRaw::Wildcard | TypeExprRaw::And(_) => return Some(entity_type.clone()), // clone: same
    };
    let mut covered = leaves
        .into_iter()
        .filter(|name| selectors.iter().any(|selector| selector_covers_type(selector, name)))
        .cloned() // clone: the narrowed selector owns its names
        .map(TypeExprRaw::Type)
        .collect::<Vec<_>>();
    match covered.len() {
        0 => None,
        1 => covered.pop(),
        _ => Some(TypeExprRaw::Or(covered)),
    }
}

/// Whether a registered type selection (Table 5.2.8-1: one name or several)
/// admits the client's compact or expanded type name. Registered names are
/// stored expanded, so a compact client name is compared against the last
/// segment of the registered IRI as well.
fn selector_covers_type(selector: &EntityTypeSelection, name: &EntityTypeName) -> bool {
    selector
        .names()
        .iter()
        .any(|registered| registered == name || compact_type_name(registered).as_ref() == Some(name))
}

/// The last fragment or path segment of an expanded type IRI, as the compact
/// name a client most likely wrote for it.
fn compact_type_name(name: &EntityTypeName) -> Option<EntityTypeName> {
    let iri = name.as_ref();
    let compact = match iri.rfind('#') {
        Some(position) => &iri[position + 1..],
        None => iri.rfind('/').map(|position| &iri[position + 1..])?,
    };
    compact.parse().ok()
}

/// The attributes among `attrs` the registration declares, or every one of
/// them when it declares no attribute names at all (clause 5.2.10: a block
/// without names covers every attribute of its entities); `None` when it
/// declares names and none of the requested ones is among them.
fn covered_attrs(attrs: &[AttributeName], registration: &ContextSourceRegistration) -> Option<Vec<AttributeName>> {
    let declared = registration
        .coverage
        .information
        .iter()
        .flat_map(|information| information.attribute_names())
        .collect::<Vec<_>>();
    if declared.is_empty() {
        return Some(attrs.to_vec()); // clone: the forwarded query owns the names it writes
    }
    let covered = attrs
        .iter()
        .filter(|attribute| {
            declared
                .iter()
                .any(|name| *name == *attribute || compact_attribute_name(name).as_ref() == Some(attribute))
        })
        .cloned() // clone: same
        .collect::<Vec<_>>();
    (!covered.is_empty()).then_some(covered)
}

#[cfg(test)]
mod tests {
    use crate::{
        config::SplitEntities,
        forwarded_parameter::ForwardedParameter,
        forwarded_query::{ClientQuery, ForwardedQuery},
        request::SourcePage,
    };
    use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
    use andromeda_ngsi_ld_request::{
        entity::GetEntityParams,
        query::{entity::parameters::EntityQueryParameters, temporal::TemporalQueryParameters},
    };
    use serde_json::{Value, json};

    fn registration(information: Value) -> ContextSourceRegistration {
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

    fn retrieve(raw: &str) -> GetEntityParams {
        serde_urlencoded::from_str(raw).expect("valid retrieve parameters")
    }

    const VEHICLE: &str = "https://uri.etsi.org/ngsi-ld/default-context/Vehicle";
    const SPEED: &str = "https://uri.etsi.org/ngsi-ld/default-context/speed";

    /// Clause 4.3.6.1: a registration for one type narrows a multi-type query to
    /// that type; one for one id and one attribute narrows the ids and
    /// attributes the same way.
    #[test]
    fn the_forwarded_query_is_narrowed_to_the_registered_types_ids_and_attributes() {
        let by_type = registration(json!([{"entities": [{"type": VEHICLE}]}]));
        let query = entity_query("type=Vehicle,Device");
        let forwarded = ForwardedQuery::for_registration(ClientQuery::EntityQuery(&query), &by_type, SplitEntities::Complete).expect("covered");
        pretty_assertions::assert_eq!(forwarded.value_of(ForwardedParameter::Type), Some("Vehicle"));

        let by_id = registration(json!([{"entities": [{"id": "urn:ngsi-ld:Vehicle:A1", "type": VEHICLE}], "propertyNames": [SPEED]}]));
        let query = entity_query("type=Vehicle&id=urn:ngsi-ld:Vehicle:A1,urn:ngsi-ld:Vehicle:A2&attrs=speed,fuel");
        let forwarded = ForwardedQuery::for_registration(ClientQuery::EntityQuery(&query), &by_id, SplitEntities::Complete).expect("covered");
        pretty_assertions::assert_eq!(forwarded.value_of(ForwardedParameter::Id), Some("urn:ngsi-ld:Vehicle:A1"));
        pretty_assertions::assert_eq!(forwarded.value_of(ForwardedParameter::Attrs), Some("speed"));
        pretty_assertions::assert_eq!(
            forwarded.to_query_string(),
            "id=urn%3Angsi-ld%3AVehicle%3AA1&type=Vehicle&limit=1000&attrs=speed"
        );
    }

    /// A registration covering none of the requested types, ids or attributes
    /// has nothing to answer and is not written a query at all.
    #[test]
    fn a_registration_covering_nothing_requested_yields_no_query() {
        let by_type = registration(json!([{"entities": [{"type": VEHICLE}]}]));
        assert!(ForwardedQuery::for_registration(ClientQuery::EntityQuery(&entity_query("type=Device")), &by_type, SplitEntities::Complete).is_none());

        let by_attr = registration(json!([{"propertyNames": [SPEED]}]));
        assert!(ForwardedQuery::for_registration(ClientQuery::EntityRetrieve(&retrieve("attrs=fuel")), &by_attr, SplitEntities::Complete).is_none());

        let by_id = registration(json!([{"entities": [{"id": "urn:ngsi-ld:Vehicle:A1", "type": VEHICLE}]}]));
        let query = entity_query("type=Vehicle&id=urn:ngsi-ld:Vehicle:A2");
        assert!(ForwardedQuery::for_registration(ClientQuery::EntityQuery(&query), &by_id, SplitEntities::Complete).is_none());
    }

    /// A registration that constrains no types, ids or attributes forwards the
    /// client's selection as written, including an `idPattern`.
    #[test]
    fn an_unrestricted_registration_forwards_the_selection_unchanged() {
        let unrestricted = registration(json!([{"propertyNames": [SPEED]}, {"entities": [{"type": VEHICLE}]}]));
        let query = entity_query("type=Vehicle,Device&id=urn:ngsi-ld:Vehicle:A1&idPattern=.*A.*&attrs=speed");
        let forwarded = ForwardedQuery::for_registration(ClientQuery::EntityQuery(&query), &unrestricted, SplitEntities::Complete).expect("covered");
        pretty_assertions::assert_eq!(forwarded.value_of(ForwardedParameter::Type), Some("Vehicle,Device"));
        pretty_assertions::assert_eq!(forwarded.value_of(ForwardedParameter::Id), Some("urn:ngsi-ld:Vehicle:A1"));
        pretty_assertions::assert_eq!(forwarded.value_of(ForwardedParameter::IdPattern), Some(".*A.*"));
    }

    /// Clause 5.7.2.4: under split entities the value, geo, scope and attribute
    /// filters are removed before forwarding, while `type`, `csf` and
    /// `datasetId` still travel.
    #[test]
    fn split_entities_strip_the_attribute_reading_filters_before_forwarding() {
        let unrestricted = registration(json!([{"entities": [{"type": VEHICLE}]}]));
        let query = entity_query(
            "type=Vehicle&q=speed%3E50&georel=near%3BmaxDistance%3D%3D1000&geometry=Point&coordinates=%5B8.0%2C44.0%5D&scopeQ=%2FMadrid&attrs=speed&csf=mode%3D%3Dinclusive&datasetId=%40none",
        );
        let forwarded = ForwardedQuery::for_registration(ClientQuery::EntityQuery(&query), &unrestricted, SplitEntities::Split).expect("covered");
        let written = forwarded.to_query_string();
        for stripped in ["q=", "georel", "geometry", "coordinates", "scopeQ", "attrs="] {
            assert!(!written.contains(stripped), "{stripped} must be stripped under split entities: {written}");
        }
        pretty_assertions::assert_eq!(forwarded.value_of(ForwardedParameter::Type), Some("Vehicle"));
        // The filter is written back in its canonical clause 4.9 text: a bare
        // token is a string literal, spelled as one.
        pretty_assertions::assert_eq!(forwarded.value_of(ForwardedParameter::Csf), Some("mode==\"inclusive\""));
        pretty_assertions::assert_eq!(forwarded.value_of(ForwardedParameter::DatasetId), Some("@none"));
    }

    /// Without split entities every filter is forwarded for remote application,
    /// the geo-query spelled back as its four clause 4.10 members.
    #[test]
    fn complete_entities_forward_the_value_geo_scope_and_source_filters() {
        let unrestricted = registration(json!([{"entities": [{"type": VEHICLE}]}]));
        let query = entity_query(
            "type=Vehicle&q=speed%3E50&georel=near%3BmaxDistance%3D%3D1000&geometry=Point&coordinates=%5B8.0%2C44.0%5D&geoproperty=location&scopeQ=%2FMadrid&attrs=speed&csf=mode%3D%3Dinclusive&lang=en&options=sysAttrs",
        );
        let forwarded = ForwardedQuery::for_registration(ClientQuery::EntityQuery(&query), &unrestricted, SplitEntities::Complete).expect("covered");
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
        let unrestricted = registration(json!([{"entities": [{"type": VEHICLE}]}]));
        let query = entity_query("type=Vehicle&format=keyValues&options=concise");
        let forwarded = ForwardedQuery::for_registration(ClientQuery::EntityQuery(&query), &unrestricted, SplitEntities::Complete).expect("covered");
        pretty_assertions::assert_eq!(forwarded.to_query_string(), "type=Vehicle&limit=1000");
    }

    #[test]
    fn an_explicit_source_page_replaces_the_post_merge_window() {
        let unrestricted = registration(json!([{"entities": [{"type": VEHICLE}]}]));
        let query = entity_query("type=Vehicle&limit=20&offset=10");
        let forwarded = ForwardedQuery::for_registration_on_page(
            ClientQuery::EntityQuery(&query),
            &unrestricted,
            SplitEntities::Complete,
            Some(SourcePage { limit: 250, offset: 750 }),
        )
        .expect("covered");
        pretty_assertions::assert_eq!(forwarded.to_query_string(), "type=Vehicle&limit=250&offset=750");
    }

    /// Clause 5.7.4.4: a split temporal query keeps its temporal window, and a
    /// complete one carries the window, the instance limit and the aggregation
    /// members the local history is rendered with.
    #[test]
    fn a_temporal_query_keeps_its_window_under_split_and_everything_else_when_complete() {
        let unrestricted = registration(json!([{"entities": [{"type": VEHICLE}]}]));
        let query = temporal_query(
            "type=Vehicle&timerel=between&timeAt=2024-01-01T00:00:00Z&endTimeAt=2024-02-01T00:00:00Z&timeproperty=modifiedAt&lastN=2&q=speed%3E50&scopeQ=%2FMadrid&attrs=speed&options=aggregatedValues,sysAttrs&aggrMethods=avg,max&aggrPeriodDuration=PT1H&csf=tier%3D%3D%22gold%22",
        );
        let split = ForwardedQuery::for_registration(ClientQuery::Temporal(&query), &unrestricted, SplitEntities::Split).expect("covered");
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

        let complete = ForwardedQuery::for_registration(ClientQuery::Temporal(&query), &unrestricted, SplitEntities::Complete).expect("covered");
        pretty_assertions::assert_eq!(complete.value_of(ForwardedParameter::Q), Some("speed>50"));
        pretty_assertions::assert_eq!(complete.value_of(ForwardedParameter::ScopeQ), Some("/Madrid"));
        pretty_assertions::assert_eq!(complete.value_of(ForwardedParameter::Attrs), Some("speed"));
        pretty_assertions::assert_eq!(complete.value_of(ForwardedParameter::Options), Some("aggregatedValues,sysAttrs"));
        pretty_assertions::assert_eq!(complete.value_of(ForwardedParameter::AggrMethods), Some("avg,max"));
        pretty_assertions::assert_eq!(complete.value_of(ForwardedParameter::AggrPeriodDuration), Some("PT1H"));
    }

    /// Clause 4.5.23: a forwarded Linked Entity retrieval carries the traversal
    /// continuation and nothing the registration does not cover.
    #[test]
    fn a_retrieval_forwards_its_traversal_continuation_and_narrowed_attributes() {
        let by_attr = registration(json!([{"propertyNames": [SPEED]}]));
        let params = retrieve("attrs=speed,fuel&join=flat&joinLevel=2&containedBy=urn:ngsi-ld:Building:1,urn:ngsi-ld:Building:2&datasetId=urn:ngsi-ld:ds:1");
        let forwarded = ForwardedQuery::for_registration(ClientQuery::EntityRetrieve(&params), &by_attr, SplitEntities::Complete).expect("covered");
        pretty_assertions::assert_eq!(
            forwarded.to_query_string(),
            "attrs=speed&datasetId=urn%3Angsi-ld%3Ads%3A1&join=flat&joinLevel=2&containedBy=urn%3Angsi-ld%3ABuilding%3A1%2Curn%3Angsi-ld%3ABuilding%3A2"
        );
        let plain =
            ForwardedQuery::for_registration(ClientQuery::EntityRetrieve(&GetEntityParams::default()), &by_attr, SplitEntities::Complete).expect("covered");
        pretty_assertions::assert_eq!(plain.to_query_string(), "");
    }

    /// The wildcard and a conjunction cannot be narrowed member by member, so
    /// they are forwarded whole; an alternation is narrowed to its covered names.
    #[test]
    fn only_a_flat_alternation_of_types_is_narrowed() {
        let by_type = registration(json!([{"entities": [{"type": VEHICLE}]}, {"entities": [{"type": "https://example.org/Car"}]}]));
        let conjunction = entity_query("type=Vehicle%3BDevice");
        let forwarded = ForwardedQuery::for_registration(ClientQuery::EntityQuery(&conjunction), &by_type, SplitEntities::Complete).expect("covered");
        pretty_assertions::assert_eq!(forwarded.value_of(ForwardedParameter::Type), Some("(Vehicle;Device)"));

        let alternation = entity_query("type=Vehicle,Car,Device");
        let forwarded = ForwardedQuery::for_registration(ClientQuery::EntityQuery(&alternation), &by_type, SplitEntities::Complete).expect("covered");
        pretty_assertions::assert_eq!(forwarded.value_of(ForwardedParameter::Type), Some("Vehicle,Car"));
    }
}
