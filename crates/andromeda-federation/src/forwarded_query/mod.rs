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
    error::FederationError,
    forwarded_names::ForwardedNames,
    forwarded_parameter::ForwardedParameter,
    forwarded_query::{
        block_footprint::{AttrCoverage, BlockSelection, block_attrs, covered_type},
        client_query::{ClientQuery, TemporalTraversal},
    },
    request::{SourcePage, TemporalAnswer},
    source_walk::SOURCE_PAGE_SIZE,
    split_entities::SplitEntities,
};
use andromeda_http_headers::language::LanguageFilter;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_model::{
    registration::{ContextSourceRegistration, info::RegistrationInfo},
    registration_matching::entity_specification::EntitySpecification,
};
use andromeda_ngsi_ld_query::dataset::DatasetSelector;
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use andromeda_ngsi_ld_request::{query::entity_selector::EntitySelectorParams, representation_parameter::WrittenValue};
use std::num::NonZeroU32;
use url::form_urlencoded;

pub mod block_footprint;
pub mod client_query;
mod filter_parameters;
pub(crate) mod id_chunks;
mod temporal_parameters;

/// The `options` token asking a source for its system-generated attributes
/// (clause 4.8; Table 6.3.11-1).
const SYS_ATTRS_OPTION: &str = "sysAttrs";

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
    /// in which case the registration has nothing to answer. `source_page` is
    /// requested when a durable range consumer must walk the complete source
    /// result set, and `answer` is the representation a temporal source is
    /// asked for. Identical block queries are collapsed so a source is
    /// contacted once for the same request.
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::JsonLd`] when a client type or attribute
    /// cannot be expanded against the request context.
    pub(crate) async fn for_registration(
        client: ClientQuery<'_>,
        registration: &ContextSourceRegistration,
        split: SplitEntities,
        source_page: Option<SourcePage>,
        answer: TemporalAnswer,
        names: &ForwardedNames<'_>,
    ) -> Result<Vec<ForwardedQuery>, FederationError> {
        let selectors = client.selectors();
        let mut queries: Vec<ForwardedQuery> = Vec::new();
        for block in &registration.coverage.information {
            for selector in selectors.iter() {
                if let Some(query) = Self::for_block(client, selector, block, split, source_page, answer, names).await?
                    && !queries.contains(&query)
                {
                    queries.push(query);
                }
            }
        }
        Ok(queries)
    }

    /// The forwarded query one information block produces for one selector of
    /// the client's union (clause 5.2.33), or `None` when the block covers none
    /// of the entities or attributes that selector asked for (clause 4.3.6.3).
    /// A retrieval has one selector and takes its entity from the operation.
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::JsonLd`] when a client type or attribute
    /// cannot be expanded against the request context.
    async fn for_block(
        client: ClientQuery<'_>,
        selector: &EntitySelectorParams,
        block: &RegistrationInfo,
        split: SplitEntities,
        source_page: Option<SourcePage>,
        answer: TemporalAnswer,
        names: &ForwardedNames<'_>,
    ) -> Result<Option<ForwardedQuery>, FederationError> {
        let mut query = ForwardedQuery::default();
        match client {
            ClientQuery::EntityQuery(params) => {
                let Some(selection) = BlockSelection::of(block, selector.id.as_deref(), selector.id_pattern.as_ref(), selector.type_.as_ref(), names).await?
                else {
                    return Ok(None);
                };
                query.push_selection(selection);
                query.push_source_page(source_page);
                match split {
                    SplitEntities::Complete => {
                        if !query.push_block_attrs(block_attrs(block, params.attrs.as_deref(), names).await?) {
                            return Ok(None);
                        }
                        query.push_q(params.q.as_ref(), params.json_keys.as_deref(), names).await?;
                        query.push_geo(params.geo.0.as_ref(), names).await?;
                        query.push_optional(ForwardedParameter::ScopeQ, params.scope_q.as_ref().map(ToString::to_string));
                    }
                    // Clause 5.7.2.4: the client's attribute filter is applied to
                    // the aggregated entities, never by a source holding a part.
                    // The block's declared attribute names are a registration
                    // constraint, not a client filter, so they still travel.
                    SplitEntities::Split => {
                        if !query.push_block_attrs(block_attrs(block, None, names).await?) {
                            return Ok(None);
                        }
                    }
                }
                query.push_csf(params.csf.as_ref(), names).await?;
                query.push_dataset_ids(params.dataset_id.as_deref());
                query.push_lang(params.lang.as_ref());
                query.push_sys_attrs(params.options.as_ref().and_then(WrittenValue::if_supported).map(|options| options.sys_attrs));
            }
            ClientQuery::EntityRetrieve { id, params } => {
                // The entity is named by the caller; the block contributes only
                // its attribute constraint, and only when it covers the entity.
                if !EntitySpecification::of_entity(&[], id).matches_entity_axis(block) {
                    return Ok(None);
                }
                // Clause 5.7.1.4: where the client specified a type, the target
                // is the entity whose id "and, where specified type, is
                // equivalent", so a block covering none of the selected types
                // does not hold it, and the source is told the part it covers.
                if let Some(entity_type) = params.entity_type.as_ref() {
                    let Some(covered) = covered_type(entity_type, block, names).await? else {
                        return Ok(None);
                    };
                    query.push(ForwardedParameter::Type, covered.to_string());
                }
                if !query.push_block_attrs(block_attrs(block, params.attrs.as_deref(), names).await?) {
                    return Ok(None);
                }
                query.push_dataset_ids(params.dataset_id.as_deref());
                query.push_lang(params.lang.as_ref());
                query.push_sys_attrs(params.options.as_ref().and_then(WrittenValue::if_supported).map(|options| options.sys_attrs));
                query.push_traversal(params.join.map(|join| join.to_string()), params.join_level, params.contained_by.as_deref());
            }
            ClientQuery::TemporalQuery(params) => {
                let Some(selection) = BlockSelection::of(block, selector.id.as_deref(), selector.id_pattern.as_ref(), selector.type_.as_ref(), names).await?
                else {
                    return Ok(None);
                };
                query.push_selection(selection);
                query.push_source_page(source_page);
                match split {
                    SplitEntities::Complete => {
                        if !query.push_block_attrs(block_attrs(block, params.attrs.as_deref(), names).await?) {
                            return Ok(None);
                        }
                        query.push_q(params.q.as_ref(), params.json_keys.as_deref(), names).await?;
                        query.push_geo(params.geo.0.as_ref(), names).await?;
                        query.push_optional(ForwardedParameter::ScopeQ, params.scope_q.as_ref().map(ToString::to_string));
                    }
                    // Clause 5.7.4.4: as for entities, the attribute-reading
                    // filters wait for the aggregation; the block's declared
                    // attribute names still travel as a registration constraint.
                    SplitEntities::Split => {
                        if !query.push_block_attrs(block_attrs(block, None, names).await?) {
                            return Ok(None);
                        }
                    }
                }
                query.push_csf(params.csf.as_ref(), names).await?;
                query.push_dataset_ids(params.dataset_id.as_deref());
                query.push_lang(params.lang.as_ref());
                query.push_temporal_parameters(params, answer);
            }
            ClientQuery::TemporalRetrieve { id, params, traversal } => {
                // Table 6.19.3.1-1: the entity is named by the path and one
                // history is no collection clause 4.12 pages, so the block
                // contributes neither a selection nor a page, only its
                // attribute constraint, and only when it covers the entity.
                if !EntitySpecification::of_entity(&[], id).matches_entity_axis(block) {
                    return Ok(None);
                }
                if !query.push_block_attrs(block_attrs(block, params.attrs.as_deref(), names).await?) {
                    return Ok(None);
                }
                query.push_dataset_ids(params.dataset_id.as_deref());
                query.push_lang(params.lang.as_ref());
                query.push_temporal_parameters(params, answer);
                // Clause 4.5.23: only a continuation asks its holder to walk
                // Linked Entities; a client's own retrieval has them walked
                // here, and a holder not serving the extension refuses the
                // parameters.
                match traversal {
                    TemporalTraversal::Continued => {
                        query.push_traversal(params.join.map(|join| join.to_string()), params.join_level, params.contained_by.as_deref());
                    }
                    TemporalTraversal::WalkedHere => {}
                }
            }
        }
        Ok(Some(query))
    }

    /// Write the page `source_page` asks each source for, leaving out a zero
    /// offset, or the page-size hint of a walk that follows `next` to the end.
    fn push_source_page(&mut self, source_page: Option<SourcePage>) {
        match source_page {
            Some(page) => {
                self.push(ForwardedParameter::Limit, page.limit.to_string());
                if page.offset > 0 {
                    self.push(ForwardedParameter::Offset, page.offset.to_string());
                }
            }
            // The page-size hint of a walk that follows the source's `next`
            // pointers to the end of its answer (clause 5.5.9.1).
            None => self.push(ForwardedParameter::Limit, SOURCE_PAGE_SIZE.to_string()),
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

    /// Append one parameter after the ones already written.
    fn push(&mut self, parameter: ForwardedParameter, value: String) {
        self.pairs.push((parameter, value));
    }

    /// Append `parameter` only when `value` is present.
    fn push_optional(&mut self, parameter: ForwardedParameter, value: Option<String>) {
        if let Some(value) = value {
            self.push(parameter, value);
        }
    }

    /// Write the entity selection one block contributes: the identifiers it
    /// narrows to or supplies, then the type, then the id pattern.
    fn push_selection(&mut self, selection: BlockSelection) {
        if !selection.ids.is_empty() {
            self.push(
                ForwardedParameter::Id,
                selection.ids.iter().map(Identifier::as_str).collect::<Vec<_>>().join(","),
            );
        }
        if let Some(entity_type) = selection.entity_type {
            self.push(ForwardedParameter::Type, entity_type.to_string());
        }
        self.push_optional(ForwardedParameter::IdPattern, selection.id_pattern);
    }

    /// Write the attribute list one block contributes, answering `false` when
    /// the block declares attribute names and covers none of the ones the client
    /// asked for, so the block has nothing to answer.
    fn push_block_attrs(&mut self, coverage: AttrCoverage) -> bool {
        match coverage {
            AttrCoverage::Uncovered => false,
            AttrCoverage::Omit => true,
            AttrCoverage::Write(names) => {
                self.push(ForwardedParameter::Attrs, names.iter().map(AttributeName::as_str).collect::<Vec<_>>().join(","));
                true
            }
        }
    }

    /// Write the `datasetId` selectors comma-separated; an absent or empty list writes nothing.
    fn push_dataset_ids(&mut self, selectors: Option<&[DatasetSelector]>) {
        if let Some(selectors) = selectors.filter(|selectors| !selectors.is_empty()) {
            self.push(
                ForwardedParameter::DatasetId,
                selectors.iter().map(ToString::to_string).collect::<Vec<_>>().join(","),
            );
        }
    }

    /// Write the `lang` preferences comma-separated, when the client gave a filter.
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

    /// The clause 4.5.23 traversal continuation of a Linked Entity retrieval.
    fn push_traversal(&mut self, join: Option<String>, join_level: Option<NonZeroU32>, contained_by: Option<&[Identifier]>) {
        self.push_optional(ForwardedParameter::Join, join);
        self.push_optional(ForwardedParameter::JoinLevel, join_level.map(|level| level.to_string()));
        if let Some(held) = contained_by.filter(|held| !held.is_empty()) {
            self.push(
                ForwardedParameter::ContainedBy,
                held.iter().map(Identifier::as_str).collect::<Vec<_>>().join(","),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        forwarded_names::ForwardedNames,
        forwarded_parameter::ForwardedParameter,
        forwarded_query::{
            ForwardedQuery,
            client_query::{ClientQuery, TemporalTraversal},
        },
        forwarding_context::Rewording,
        request::{SourcePage, TemporalAnswer},
        split_entities::SplitEntities,
        term_resolver::TermResolver,
    };
    use andromeda_json_literal::JsonLiteral;
    use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
    use andromeda_ngsi_ld_request::{
        entity::GetEntityParams,
        query::{
            entity::parameters::EntityQueryParameters,
            entity_selector::EntitySelectorParams,
            temporal::{TemporalQueryParameters, retrieve_parameters::TemporalRetrieveParameters},
        },
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

    fn uri(raw: &str) -> Identifier {
        raw.parse().expect("valid URI")
    }

    fn temporal_retrieve<'a>(id: &'a Identifier, params: &'a TemporalQueryParameters, traversal: TemporalTraversal) -> ClientQuery<'a> {
        ClientQuery::TemporalRetrieve { id, params, traversal }
    }

    fn strings(queries: &[ForwardedQuery]) -> Vec<String> {
        queries.iter().map(ForwardedQuery::to_query_string).collect()
    }

    /// An engine that resolves only what is embedded, never the network.
    fn offline_engine() -> JsonLdEngine {
        JsonLdEngine::builder().allow_remote(false).build().expect("engine builds")
    }

    /// Narrow `client` under the core context, where every term equals the last segment of its IRI.
    async fn narrow(client: ClientQuery<'_>, registration: &ContextSourceRegistration, split: SplitEntities) -> Vec<ForwardedQuery> {
        let engine = offline_engine();
        let context = ContextSpec::none();
        let resolver = TermResolver::new(&engine, &context);
        let names = ForwardedNames::prepare(&engine, &resolver, &context, Rewording::AsInbound)
            .await
            .expect("the forwarding context processes");
        ForwardedQuery::for_registration(client, registration, split, None, TemporalAnswer::AsRequested, &names)
            .await
            .expect("narrowing succeeds")
    }

    /// Narrow `client` for a durable source page under the core context.
    async fn narrow_on_page(client: ClientQuery<'_>, registration: &ContextSourceRegistration, split: SplitEntities, page: SourcePage) -> Vec<ForwardedQuery> {
        let engine = offline_engine();
        let context = ContextSpec::none();
        let resolver = TermResolver::new(&engine, &context);
        let names = ForwardedNames::prepare(&engine, &resolver, &context, Rewording::AsInbound)
            .await
            .expect("the forwarding context processes");
        ForwardedQuery::for_registration(client, registration, split, Some(page), TemporalAnswer::AsRequested, &names)
            .await
            .expect("narrowing succeeds")
    }

    /// Narrow `client` under an inline `@context` that aliases its terms to unrelated IRIs, so the compact term differs from the IRI's last segment.
    async fn narrow_aliased(
        client: ClientQuery<'_>,
        registration: &ContextSourceRegistration,
        split: SplitEntities,
        body_context: JsonLiteral,
    ) -> Vec<ForwardedQuery> {
        let engine = offline_engine();
        let context = ContextSpec::from_body(body_context);
        let resolver = TermResolver::new(&engine, &context);
        let names = ForwardedNames::prepare(&engine, &resolver, &context, Rewording::AsInbound)
            .await
            .expect("the forwarding context processes");
        ForwardedQuery::for_registration(client, registration, split, None, TemporalAnswer::AsRequested, &names)
            .await
            .expect("narrowing succeeds")
    }

    const VEHICLE: &str = "https://uri.etsi.org/ngsi-ld/default-context/Vehicle";
    const DEVICE: &str = "https://uri.etsi.org/ngsi-ld/default-context/Device";
    const SPEED: &str = "https://uri.etsi.org/ngsi-ld/default-context/speed";
    const FUEL: &str = "https://uri.etsi.org/ngsi-ld/default-context/fuel";
    const VEHICLE_X: &str = "urn:ngsi-ld:Vehicle:X";

    /// The IRIs an aliased vocabulary expands its terms to, unrelated to the
    /// terms' last segments so the engine and the last-segment heuristic differ.
    const ALIASED_SPEED: &str = "https://example.org/vocab/speed";
    const ALIASED_VEHICLE: &str = "https://example.org/vocab/Vehicle";

    /// The `@context` body that spells the aliased IRIs `spd` and `Veh`.
    fn aliased_context() -> JsonLiteral {
        JsonLiteral::admit(&json!({ "spd": ALIASED_SPEED, "Veh": ALIASED_VEHICLE }).to_string()).expect("an admissible literal")
    }

    /// Clauses 4.3.6.1 and 4.3.6.3: an exclusive block names one entity and its
    /// attributes, so a client asking only for the type still has the block's id
    /// and attribute added to the forwarded request.
    #[tokio::test]
    async fn an_exclusive_block_forwards_its_id_and_attribute_for_a_type_only_query() {
        let block = registration(&json!([{"entities": [{"id": VEHICLE_X, "type": VEHICLE}], "propertyNames": [SPEED]}]));
        let query = entity_query("type=Vehicle");
        let forwarded = narrow(ClientQuery::EntityQuery(&query), &block, SplitEntities::Complete).await;
        pretty_assertions::assert_eq!(
            strings(&forwarded),
            vec!["id=urn%3Angsi-ld%3AVehicle%3AX&type=Vehicle&limit=1000&attrs=speed".to_string()]
        );
    }

    /// Clause 4.3.6.3: two blocks each cover a different type and attribute, so
    /// each forwards its own query, never one query carrying the union, which
    /// would ask a source for a cross-product it never registered.
    #[tokio::test]
    async fn two_blocks_forward_two_queries_without_a_cross_product() {
        let reg = registration(&json!([
            {"entities": [{"type": VEHICLE}], "propertyNames": [SPEED]},
            {"entities": [{"type": DEVICE}], "propertyNames": [FUEL]}
        ]));
        let forwarded = narrow(ClientQuery::EntityQuery(&entity_query("")), &reg, SplitEntities::Complete).await;
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
    #[tokio::test]
    async fn a_block_whose_attributes_the_client_did_not_request_yields_no_query() {
        let block = registration(&json!([{"propertyNames": [SPEED]}]));
        let forwarded = narrow(ClientQuery::EntityQuery(&entity_query("attrs=fuel")), &block, SplitEntities::Complete).await;
        pretty_assertions::assert_eq!(strings(&forwarded), Vec::<String>::new());
    }

    /// A block covering none of the requested types produces no forwarded query.
    #[tokio::test]
    async fn a_block_covering_none_of_the_requested_types_yields_no_query() {
        let block = registration(&json!([{"entities": [{"type": VEHICLE}]}]));
        let forwarded = narrow(ClientQuery::EntityQuery(&entity_query("type=Device")), &block, SplitEntities::Complete).await;
        pretty_assertions::assert_eq!(strings(&forwarded), Vec::<String>::new());
    }

    /// A block that declares no attribute names covers every attribute, so the
    /// client's attribute list travels unchanged.
    #[tokio::test]
    async fn a_type_only_block_forwards_the_client_attributes_unchanged() {
        let block = registration(&json!([{"entities": [{"type": VEHICLE}]}]));
        let query = entity_query("type=Vehicle&attrs=speed,fuel");
        let forwarded = narrow(ClientQuery::EntityQuery(&query), &block, SplitEntities::Complete).await;
        pretty_assertions::assert_eq!(forwarded.len(), 1);
        pretty_assertions::assert_eq!(forwarded[0].value_of(ForwardedParameter::Type), Some("Vehicle"));
        pretty_assertions::assert_eq!(forwarded[0].value_of(ForwardedParameter::Attrs), Some("speed,fuel"));
    }

    /// A block constraining identifiers by pattern forwards that pattern when the
    /// client named no identifier of its own.
    #[tokio::test]
    async fn a_block_constraining_ids_by_pattern_forwards_that_pattern() {
        let block = registration(&json!([{"entities": [{"type": VEHICLE, "idPattern": "urn:ngsi-ld:Vehicle:.*"}]}]));
        let forwarded = narrow(ClientQuery::EntityQuery(&entity_query("")), &block, SplitEntities::Complete).await;
        pretty_assertions::assert_eq!(forwarded.len(), 1);
        pretty_assertions::assert_eq!(forwarded[0].value_of(ForwardedParameter::IdPattern), Some("urn:ngsi-ld:Vehicle:.*"));
        pretty_assertions::assert_eq!(forwarded[0].value_of(ForwardedParameter::Type), Some("Vehicle"));
    }

    /// Clause 5.12, fourth bullet: a client pattern matches a block only
    /// through an `id` it matches, so a block registering another `id` is not
    /// asked, and one registering a matching `id` is asked with the pattern.
    #[tokio::test]
    async fn a_client_pattern_reaches_only_the_blocks_whose_registered_id_it_matches() {
        let query = entity_query("type=Vehicle&idPattern=urn:ngsi-ld:Vehicle:1.*");
        let other = registration(&json!([{"entities": [{"type": VEHICLE, "id": "urn:ngsi-ld:Vehicle:7"}]}]));
        let forwarded = narrow(ClientQuery::EntityQuery(&query), &other, SplitEntities::Complete).await;
        pretty_assertions::assert_eq!(strings(&forwarded), Vec::<String>::new());

        let matching = registration(&json!([{"entities": [{"type": VEHICLE, "id": "urn:ngsi-ld:Vehicle:12"}]}]));
        let forwarded = narrow(ClientQuery::EntityQuery(&query), &matching, SplitEntities::Complete).await;
        pretty_assertions::assert_eq!(forwarded.len(), 1);
        pretty_assertions::assert_eq!(forwarded[0].value_of(ForwardedParameter::IdPattern), Some("urn:ngsi-ld:Vehicle:1.*"));
        pretty_assertions::assert_eq!(forwarded[0].value_of(ForwardedParameter::Type), Some("Vehicle"));
    }

    /// Clause 5.12, first and fifth bullets: a block constraining the
    /// identifier not at all, by a pattern of its own, or naming no entity
    /// selector at all may hold an entity any pattern matches.
    #[tokio::test]
    async fn a_client_pattern_reaches_a_block_registering_a_pattern_or_no_identifier() {
        let query = entity_query("type=Vehicle&idPattern=urn:ngsi-ld:Vehicle:1.*");
        for information in [
            json!([{"entities": [{"type": VEHICLE, "idPattern": "urn:ngsi-ld:Fleet:.*"}]}]),
            json!([{"entities": [{"type": VEHICLE}]}]),
            json!([{"propertyNames": [SPEED]}]),
        ] {
            let block = registration(&information);
            let forwarded = narrow(ClientQuery::EntityQuery(&query), &block, SplitEntities::Complete).await;
            pretty_assertions::assert_eq!(forwarded.len(), 1, "{information}");
            pretty_assertions::assert_eq!(
                forwarded[0].value_of(ForwardedParameter::IdPattern),
                Some("urn:ngsi-ld:Vehicle:1.*"),
                "{information}"
            );
        }
    }

    /// Clause 5.12: one `EntityInfo` of a block matching is enough, and only
    /// the blocks with one are asked.
    #[tokio::test]
    async fn a_client_pattern_is_decided_per_entity_selector_and_per_block() {
        let query = entity_query("idPattern=urn:ngsi-ld:Vehicle:1.*");
        let block = registration(&json!([
            {"entities": [{"type": VEHICLE, "id": "urn:ngsi-ld:Vehicle:7"}, {"type": VEHICLE, "id": "urn:ngsi-ld:Vehicle:12"}], "propertyNames": [SPEED]},
            {"entities": [{"type": VEHICLE, "id": "urn:ngsi-ld:Vehicle:8"}], "propertyNames": [FUEL]}
        ]));
        let forwarded = narrow(ClientQuery::EntityQuery(&query), &block, SplitEntities::Complete).await;
        pretty_assertions::assert_eq!(forwarded.len(), 1);
        pretty_assertions::assert_eq!(forwarded[0].value_of(ForwardedParameter::Attrs), Some("speed"));
    }

    /// Clause 5.2.8: a registered `id` takes precedence over the `idPattern`
    /// beside it, so the client pattern is matched against the `id` alone.
    #[tokio::test]
    async fn a_client_pattern_is_matched_against_a_registered_id_before_the_pattern_beside_it() {
        let query = entity_query("type=Vehicle&idPattern=urn:ngsi-ld:Vehicle:1.*");
        let block = registration(&json!([{"entities": [{"type": VEHICLE, "id": "urn:ngsi-ld:Vehicle:7", "idPattern": ".*"}]}]));
        let forwarded = narrow(ClientQuery::EntityQuery(&query), &block, SplitEntities::Complete).await;
        pretty_assertions::assert_eq!(strings(&forwarded), Vec::<String>::new());
    }

    /// Clause 5.7.4.4 matches a temporal query against registrations as clause
    /// 5.7.2.4 does an entity query.
    #[tokio::test]
    async fn a_temporal_client_pattern_skips_a_block_whose_registered_id_it_does_not_match() {
        let query = temporal_query("type=Vehicle&idPattern=urn:ngsi-ld:Vehicle:1.*&timerel=after&timeAt=2024-01-01T00:00:00Z");
        let other = registration(&json!([{"entities": [{"type": VEHICLE, "id": "urn:ngsi-ld:Vehicle:7"}]}]));
        let forwarded = narrow(ClientQuery::TemporalQuery(&query), &other, SplitEntities::Complete).await;
        pretty_assertions::assert_eq!(strings(&forwarded), Vec::<String>::new());
        let matching = registration(&json!([{"entities": [{"type": VEHICLE, "id": "urn:ngsi-ld:Vehicle:12"}]}]));
        let forwarded = narrow(ClientQuery::TemporalQuery(&query), &matching, SplitEntities::Complete).await;
        pretty_assertions::assert_eq!(forwarded.len(), 1);
    }

    /// Clause 5.7.2.4: under split entities the client's value, geo, scope and
    /// attribute filters are removed before forwarding, but the block's declared
    /// attribute names still travel as a registration constraint.
    #[tokio::test]
    async fn split_entities_drop_the_client_filters_but_keep_the_block_attributes() {
        let block = registration(&json!([{"entities": [{"type": VEHICLE}], "propertyNames": [SPEED]}]));
        let query = entity_query(
            "type=Vehicle&q=speed%3E50&georel=near%3BmaxDistance%3D%3D1000&geometry=Point&coordinates=%5B8.0%2C44.0%5D&scopeQ=%2FMadrid&attrs=fuel&csf=mode%3D%3D%22inclusive%22&datasetId=%40none",
        );
        let forwarded = narrow(ClientQuery::EntityQuery(&query), &block, SplitEntities::Split).await;
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
    #[tokio::test]
    async fn complete_entities_forward_the_value_geo_scope_and_source_filters() {
        let block = registration(&json!([{"entities": [{"type": VEHICLE}]}]));
        let query = entity_query(
            "type=Vehicle&q=speed%3E50&georel=near%3BmaxDistance%3D%3D1000&geometry=Point&coordinates=%5B8.0%2C44.0%5D&geoproperty=location&scopeQ=%2FMadrid&attrs=speed&csf=mode%3D%3D%22inclusive%22&lang=en&options=sysAttrs",
        );
        let forwarded = narrow(ClientQuery::EntityQuery(&query), &block, SplitEntities::Complete).await;
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
    #[tokio::test]
    async fn a_representation_choice_is_never_forwarded() {
        let block = registration(&json!([{"entities": [{"type": VEHICLE}]}]));
        let query = entity_query("type=Vehicle&format=keyValues&options=concise");
        let forwarded = narrow(ClientQuery::EntityQuery(&query), &block, SplitEntities::Complete).await;
        pretty_assertions::assert_eq!(strings(&forwarded), vec!["type=Vehicle&limit=1000".to_string()]);
    }

    #[tokio::test]
    async fn an_explicit_source_page_replaces_the_post_merge_window() {
        let block = registration(&json!([{"entities": [{"type": VEHICLE}]}]));
        let query = entity_query("type=Vehicle&limit=20&offset=10");
        let forwarded = narrow_on_page(
            ClientQuery::EntityQuery(&query),
            &block,
            SplitEntities::Complete,
            SourcePage { limit: 250, offset: 750 },
        )
        .await;
        pretty_assertions::assert_eq!(strings(&forwarded), vec!["type=Vehicle&limit=250&offset=750".to_string()]);
    }

    /// Two identical blocks produce the same forwarded query, which is issued
    /// once: a source is not contacted twice for the same request.
    #[tokio::test]
    async fn identical_block_queries_are_deduplicated() {
        let reg = registration(&json!([{"entities": [{"type": VEHICLE}]}, {"entities": [{"type": VEHICLE}]}]));
        let forwarded = narrow(ClientQuery::EntityQuery(&entity_query("type=Vehicle")), &reg, SplitEntities::Complete).await;
        pretty_assertions::assert_eq!(strings(&forwarded), vec!["type=Vehicle&limit=1000".to_string()]);
    }

    /// A POST query naming `selectors`, whose flat members are the flattened
    /// summary the body conversion leaves.
    fn union_query(selectors: Vec<EntitySelectorParams>) -> EntityQueryParameters {
        let mut query = entity_query("");
        query.entity_selectors = selectors;
        query
    }

    fn typed(raw: &str) -> EntitySelectorParams {
        EntitySelectorParams {
            type_: Some(raw.parse().expect("valid type expression")),
            ..EntitySelectorParams::default()
        }
    }

    /// Clause 5.2.33: a block is asked only for the selectors of the union it
    /// covers, so a block registered for `Device` alone yields the `Device`
    /// query and nothing for `Vehicle`.
    #[tokio::test]
    async fn a_block_is_asked_only_for_the_selectors_it_covers() {
        let reg = registration(&json!([{"entities": [{"type": DEVICE}]}]));
        let query = union_query(vec![typed("Vehicle"), typed("Device")]);
        let forwarded = narrow(ClientQuery::EntityQuery(&query), &reg, SplitEntities::Complete).await;
        pretty_assertions::assert_eq!(strings(&forwarded), vec!["type=Device&limit=1000".to_string()]);
    }

    /// A block covering both selectors is asked once per selector, since a GET
    /// cannot express their union.
    #[tokio::test]
    async fn a_block_covering_two_selectors_is_asked_once_for_each() {
        let reg = registration(&json!([{"entities": [{"type": VEHICLE}, {"type": DEVICE}]}]));
        let query = union_query(vec![typed("Vehicle"), typed("Device")]);
        let forwarded = narrow(ClientQuery::EntityQuery(&query), &reg, SplitEntities::Complete).await;
        pretty_assertions::assert_eq!(
            strings(&forwarded),
            vec!["type=Vehicle&limit=1000".to_string(), "type=Device&limit=1000".to_string()]
        );
    }

    /// Two selectors that narrow to the same request collapse to one query.
    #[tokio::test]
    async fn selectors_narrowing_to_the_same_request_are_asked_once() {
        let reg = registration(&json!([{"entities": [{"type": VEHICLE}]}]));
        let query = union_query(vec![typed("Vehicle"), typed("Vehicle")]);
        let forwarded = narrow(ClientQuery::EntityQuery(&query), &reg, SplitEntities::Complete).await;
        pretty_assertions::assert_eq!(strings(&forwarded), vec!["type=Vehicle&limit=1000".to_string()]);
    }

    /// A registration matching none of the selectors yields no query, so no
    /// request is sent to its source.
    #[tokio::test]
    async fn a_block_covering_none_of_the_selectors_yields_no_query() {
        let reg = registration(&json!([{"entities": [{"type": "https://uri.etsi.org/ngsi-ld/default-context/Other"}]}]));
        let query = union_query(vec![typed("Vehicle"), typed("Device")]);
        let forwarded = narrow(ClientQuery::EntityQuery(&query), &reg, SplitEntities::Complete).await;
        pretty_assertions::assert_eq!(strings(&forwarded), Vec::<String>::new());
    }

    /// The identifier and the type of one selector are decided together: a
    /// block registering the identifier under another type is not asked.
    #[tokio::test]
    async fn a_selector_pairs_its_identifier_with_its_own_type() {
        let reg = registration(&json!([{"entities": [{"type": DEVICE, "id": VEHICLE_X}]}]));
        let query = union_query(vec![EntitySelectorParams {
            id: Some(vec![uri(VEHICLE_X)]),
            ..typed("Vehicle")
        }]);
        let forwarded = narrow(ClientQuery::EntityQuery(&query), &reg, SplitEntities::Complete).await;
        pretty_assertions::assert_eq!(strings(&forwarded), Vec::<String>::new());
    }

    /// The union applies to a temporal query as it does to an entity query.
    #[tokio::test]
    async fn a_temporal_query_is_asked_only_for_the_selectors_a_block_covers() {
        let reg = registration(&json!([{"entities": [{"type": DEVICE}]}]));
        let query = TemporalQueryParameters {
            entity_selectors: vec![typed("Vehicle"), typed("Device")],
            ..TemporalQueryParameters::default()
        };
        let forwarded = narrow(ClientQuery::TemporalQuery(&query), &reg, SplitEntities::Complete).await;
        pretty_assertions::assert_eq!(forwarded.len(), 1);
        pretty_assertions::assert_eq!(forwarded[0].value_of(ForwardedParameter::Type), Some("Device"));
    }

    /// Clause 5.7.4.4: a split temporal query keeps its temporal window, and a
    /// complete one carries the window, the instance limit and the aggregation
    /// members the local history is rendered with.
    #[tokio::test]
    async fn a_temporal_query_keeps_its_window_under_split_and_everything_else_when_complete() {
        let block = registration(&json!([{"entities": [{"type": VEHICLE}]}]));
        let query = temporal_query(
            "type=Vehicle&timerel=between&timeAt=2024-01-01T00:00:00Z&endTimeAt=2024-02-01T00:00:00Z&timeproperty=modifiedAt&lastN=2&q=speed%3E50&scopeQ=%2FMadrid&attrs=speed&options=aggregatedValues,sysAttrs&aggrMethods=avg,max&aggrPeriodDuration=PT1H&csf=tier%3D%3D%22gold%22",
        );
        let split = narrow(ClientQuery::TemporalQuery(&query), &block, SplitEntities::Split).await;
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

        let complete = narrow(ClientQuery::TemporalQuery(&query), &block, SplitEntities::Complete).await;
        pretty_assertions::assert_eq!(complete.len(), 1);
        let complete = &complete[0];
        pretty_assertions::assert_eq!(complete.value_of(ForwardedParameter::Q), Some("speed>50"));
        pretty_assertions::assert_eq!(complete.value_of(ForwardedParameter::ScopeQ), Some("/Madrid"));
        pretty_assertions::assert_eq!(complete.value_of(ForwardedParameter::Attrs), Some("speed"));
        pretty_assertions::assert_eq!(complete.value_of(ForwardedParameter::Format), Some("aggregatedValues"));
        pretty_assertions::assert_eq!(complete.value_of(ForwardedParameter::Options), Some("sysAttrs"));
        pretty_assertions::assert_eq!(complete.value_of(ForwardedParameter::AggrMethods), Some("avg,max"));
        pretty_assertions::assert_eq!(complete.value_of(ForwardedParameter::AggrPeriodDuration), Some("PT1H"));
    }

    /// Clause 4.3.6.3: a per-block temporal query is narrowed the same way an
    /// entity query is (the block's id and attribute are named even though the
    /// client asked only for the type).
    #[tokio::test]
    async fn a_temporal_query_is_narrowed_per_block_like_an_entity_query() {
        let block = registration(&json!([{"entities": [{"id": VEHICLE_X, "type": VEHICLE}], "propertyNames": [SPEED]}]));
        let query = temporal_query("type=Vehicle&timerel=before&timeAt=2030-01-01T00:00:00Z");
        let forwarded = narrow(ClientQuery::TemporalQuery(&query), &block, SplitEntities::Complete).await;
        pretty_assertions::assert_eq!(forwarded.len(), 1);
        pretty_assertions::assert_eq!(forwarded[0].value_of(ForwardedParameter::Id), Some(VEHICLE_X));
        pretty_assertions::assert_eq!(forwarded[0].value_of(ForwardedParameter::Type), Some("Vehicle"));
        pretty_assertions::assert_eq!(forwarded[0].value_of(ForwardedParameter::Attrs), Some("speed"));
        pretty_assertions::assert_eq!(forwarded[0].value_of(ForwardedParameter::Timerel), Some("before"));
    }

    /// Clause 4.5.23: a forwarded Linked Entity retrieval carries the traversal
    /// continuation, and the block declaring an attribute adds it even when the
    /// client named none.
    #[tokio::test]
    async fn a_retrieval_forwards_its_traversal_continuation_and_the_block_attribute() {
        let block = registration(&json!([{"entities": [{"id": VEHICLE_X, "type": VEHICLE}], "propertyNames": [SPEED]}]));
        let id = uri(VEHICLE_X);
        let params: GetEntityParams =
            serde_urlencoded::from_str("join=flat&joinLevel=2&containedBy=urn:ngsi-ld:Building:1,urn:ngsi-ld:Building:2&datasetId=urn:ngsi-ld:ds:1")
                .expect("valid retrieve parameters");
        let forwarded = narrow(ClientQuery::EntityRetrieve { id: &id, params: &params }, &block, SplitEntities::Complete).await;
        pretty_assertions::assert_eq!(
            strings(&forwarded),
            vec![
                "attrs=speed&datasetId=urn%3Angsi-ld%3Ads%3A1&join=flat&joinLevel=2&containedBy=urn%3Angsi-ld%3ABuilding%3A1%2Curn%3Angsi-ld%3ABuilding%3A2"
                    .to_string()
            ]
        );
    }

    /// Clause 4.5.23 under `temporal_linked_retrieval`: a client's own
    /// temporal query or retrieval has its Linked Entities walked by this
    /// broker, so no source is sent `join`, `joinLevel` or `containedBy`; the
    /// continuation of a walk is, to the holder that serves it, and as a
    /// retrieval it still names no selection and no page.
    #[tokio::test]
    async fn only_a_temporal_retrieval_continuation_forwards_the_traversal() {
        let block = registration(&json!([{"entities": [{"type": VEHICLE}]}]));
        let traversal = "timerel=after&timeAt=2020-01-01T00:00:00Z&join=inline&joinLevel=2&containedBy=urn:ngsi-ld:Building:1";
        let query = temporal_query(&format!("type=Vehicle&{traversal}"));
        let client = strings(&narrow(ClientQuery::TemporalQuery(&query), &block, SplitEntities::Split).await);
        let [client] = client.as_slice() else {
            panic!("one block, one query: {client:?}");
        };
        for parameter in ["join=", "joinLevel=", "containedBy="] {
            assert!(!client.contains(parameter), "{parameter} travels: {client}");
        }

        let retrieval = temporal_query(traversal);
        let id = uri(VEHICLE_X);
        let walked_here = strings(
            &narrow(
                temporal_retrieve(&id, &retrieval, TemporalTraversal::WalkedHere),
                &block,
                SplitEntities::Complete,
            )
            .await,
        );
        pretty_assertions::assert_eq!(walked_here, vec!["timerel=after&timeAt=2020-01-01T00%3A00%3A00Z".to_string()]);

        let continuation = strings(
            &narrow(
                temporal_retrieve(&id, &retrieval, TemporalTraversal::Continued),
                &block,
                SplitEntities::Complete,
            )
            .await,
        );
        pretty_assertions::assert_eq!(
            continuation,
            vec!["timerel=after&timeAt=2020-01-01T00%3A00%3A00Z&join=inline&joinLevel=2&containedBy=urn%3Angsi-ld%3ABuilding%3A1".to_string()]
        );
    }

    /// Table 6.19.3.1-1: the entity is named by the path and one history is no
    /// collection clause 4.12 pages, so a block registering a type and a
    /// pattern adds neither to the forwarded retrieval, nor a page size.
    #[tokio::test]
    async fn a_temporal_retrieval_forwards_no_selection_and_no_page() {
        let block = registration(&json!([{"entities": [{"type": VEHICLE, "idPattern": "urn:ngsi-ld:Vehicle:.*"}]}]));
        let id = uri(VEHICLE_X);
        let params = temporal_query("timerel=before&timeAt=2030-01-01T00:00:00Z");
        let forwarded = narrow(temporal_retrieve(&id, &params, TemporalTraversal::WalkedHere), &block, SplitEntities::Complete).await;
        pretty_assertions::assert_eq!(strings(&forwarded), vec!["timerel=before&timeAt=2030-01-01T00%3A00%3A00Z".to_string()]);
    }

    /// Clause 4.3.6.1: the block's attribute constraint narrows the client's
    /// `attrs`, and the dataset, language and window members Table 6.19.3.1-1
    /// defines travel unchanged.
    #[tokio::test]
    async fn a_temporal_retrieval_forwards_the_block_attribute_dataset_lang_and_window() {
        let block = registration(&json!([{"entities": [{"id": VEHICLE_X, "type": VEHICLE}], "propertyNames": [SPEED]}]));
        let id = uri(VEHICLE_X);
        let params = temporal_query("attrs=speed,fuel&datasetId=urn:ngsi-ld:ds:1&lang=en&timerel=after&timeAt=2024-01-01T00:00:00Z&lastN=2");
        let forwarded = narrow(temporal_retrieve(&id, &params, TemporalTraversal::WalkedHere), &block, SplitEntities::Complete).await;
        pretty_assertions::assert_eq!(
            strings(&forwarded),
            vec!["attrs=speed&datasetId=urn%3Angsi-ld%3Ads%3A1&lang=en&timerel=after&timeAt=2024-01-01T00%3A00%3A00Z&lastN=2".to_string()]
        );
    }

    /// A temporal retrieval whose target the block does not cover produces no
    /// forwarded retrieval for that block.
    #[tokio::test]
    async fn a_temporal_retrieval_skips_a_block_that_does_not_cover_the_entity() {
        let block = registration(&json!([{"entities": [{"id": "urn:ngsi-ld:Vehicle:Other", "type": VEHICLE}], "propertyNames": [SPEED]}]));
        let id = uri(VEHICLE_X);
        let params = temporal_query("timerel=before&timeAt=2030-01-01T00:00:00Z");
        let forwarded = narrow(temporal_retrieve(&id, &params, TemporalTraversal::WalkedHere), &block, SplitEntities::Complete).await;
        pretty_assertions::assert_eq!(strings(&forwarded), Vec::<String>::new());
    }

    /// Clause 6.3.20: the temporal query model the retrieval is read into can
    /// hold members Table 6.19.3.1-1 does not define, and none of them is ever
    /// written into the forwarded retrieval.
    #[tokio::test]
    async fn a_temporal_retrieval_never_writes_a_query_only_parameter() {
        let block = registration(&json!([{"entities": [{"type": VEHICLE}]}]));
        let id = uri(VEHICLE_X);
        let params = temporal_query(
            "type=Vehicle&id=urn:ngsi-ld:Vehicle:X&idPattern=urn:ngsi-ld:Vehicle:.*&q=speed%3E1&scopeQ=%2FMadrid&csf=tier%3D%3D1&limit=3&timerel=after&timeAt=2024-01-01T00:00:00Z",
        );
        let forwarded = strings(&narrow(temporal_retrieve(&id, &params, TemporalTraversal::WalkedHere), &block, SplitEntities::Complete).await);
        pretty_assertions::assert_eq!(forwarded, vec!["timerel=after&timeAt=2024-01-01T00%3A00%3A00Z".to_string()]);
    }

    /// The forwarded retrieval is admitted by the receiving binding, whatever
    /// representation, projection or continuation the client asked for.
    #[tokio::test]
    async fn a_forwarded_temporal_retrieval_is_admitted_by_the_retrieval_binding() {
        let block = registration(&json!([{"entities": [{"id": VEHICLE_X, "type": VEHICLE}], "propertyNames": [SPEED]}]));
        let id = uri(VEHICLE_X);
        for (raw, traversal) in [
            (
                "attrs=speed&datasetId=@none&lang=en&timerel=after&timeAt=2024-01-01T00:00:00Z&lastN=2&options=sysAttrs",
                TemporalTraversal::WalkedHere,
            ),
            (
                "timerel=between&timeAt=2024-01-01T00:00:00Z&endTimeAt=2024-02-01T00:00:00Z&timeproperty=modifiedAt&format=aggregatedValues&aggrMethods=avg,max&aggrPeriodDuration=PT1H",
                TemporalTraversal::WalkedHere,
            ),
            (
                "timerel=after&timeAt=2024-01-01T00:00:00Z&join=flat&joinLevel=2&containedBy=urn:ngsi-ld:Building:1",
                TemporalTraversal::Continued,
            ),
        ] {
            let params = temporal_query(raw);
            let forwarded = strings(&narrow(temporal_retrieve(&id, &params, traversal), &block, SplitEntities::Complete).await);
            let [forwarded] = forwarded.as_slice() else {
                panic!("one block, one retrieval: {forwarded:?}");
            };
            serde_urlencoded::from_str::<TemporalRetrieveParameters>(forwarded).unwrap_or_else(|error| panic!("{forwarded}: {error}"));
        }
    }

    /// A retrieval whose target the block does not cover produces no forwarded
    /// query for that block.
    #[tokio::test]
    async fn a_retrieval_skips_a_block_that_does_not_cover_the_entity() {
        let block = registration(&json!([{"entities": [{"id": "urn:ngsi-ld:Vehicle:Other", "type": VEHICLE}], "propertyNames": [SPEED]}]));
        let id = uri(VEHICLE_X);
        let params = GetEntityParams::default();
        let forwarded = narrow(ClientQuery::EntityRetrieve { id: &id, params: &params }, &block, SplitEntities::Complete).await;
        pretty_assertions::assert_eq!(strings(&forwarded), Vec::<String>::new());
    }

    /// Clauses 5.7.1.3 and 5.7.1.4: a retrieval naming a type reaches a block
    /// only when the block covers the entity under one of the selected types,
    /// and the source is told the type so it applies the same selection.
    #[tokio::test]
    async fn a_typed_retrieval_forwards_the_type_the_block_covers() {
        let block = registration(&json!([{"entities": [{"id": VEHICLE_X, "type": VEHICLE}]}]));
        let id = uri(VEHICLE_X);
        let params: GetEntityParams = serde_urlencoded::from_str("type=Vehicle").expect("valid retrieve parameters");
        let forwarded = narrow(ClientQuery::EntityRetrieve { id: &id, params: &params }, &block, SplitEntities::Complete).await;
        pretty_assertions::assert_eq!(strings(&forwarded), vec!["type=Vehicle".to_string()]);
    }

    /// Clause 5.7.1.4: a block registering the entity under a type outside the
    /// selector does not hold the target, so it forwards nothing.
    #[tokio::test]
    async fn a_typed_retrieval_skips_a_block_registering_another_type() {
        let block = registration(&json!([{"entities": [{"id": VEHICLE_X, "type": "https://example.org/Car"}]}]));
        let id = uri(VEHICLE_X);
        let params: GetEntityParams = serde_urlencoded::from_str("type=Vehicle").expect("valid retrieve parameters");
        let forwarded = narrow(ClientQuery::EntityRetrieve { id: &id, params: &params }, &block, SplitEntities::Complete).await;
        pretty_assertions::assert_eq!(strings(&forwarded), Vec::<String>::new());
    }

    /// The wildcard and a conjunction cannot be narrowed member by member, so
    /// they are forwarded whole; an alternation is narrowed per block, so each
    /// covering block forwards its own covered name.
    #[tokio::test]
    async fn only_a_flat_alternation_of_types_is_narrowed_per_block() {
        let reg = registration(&json!([{"entities": [{"type": VEHICLE}]}, {"entities": [{"type": "https://example.org/Car"}]}]));
        let conjunction = entity_query("type=%28Vehicle%3BDevice%29");
        let forwarded = narrow(ClientQuery::EntityQuery(&conjunction), &reg, SplitEntities::Complete).await;
        // Both blocks forward the conjunction whole, so the two queries are
        // identical and collapse to one.
        pretty_assertions::assert_eq!(forwarded.len(), 1);
        pretty_assertions::assert_eq!(forwarded[0].value_of(ForwardedParameter::Type), Some("(Vehicle;Device)"));

        // The alternation names Car by its aliased IRI, so the request context
        // spells it under that alias for the leaf to compact back to `Car`.
        let alternation = entity_query("type=Vehicle,Car,Device");
        let forwarded = narrow_aliased(
            ClientQuery::EntityQuery(&alternation),
            &reg,
            SplitEntities::Complete,
            JsonLiteral::admit(r#"{"Car": "https://example.org/Car"}"#).expect("an admissible literal"),
        )
        .await;
        let types = forwarded
            .iter()
            .filter_map(|query| query.value_of(ForwardedParameter::Type))
            .collect::<Vec<_>>();
        pretty_assertions::assert_eq!(types, vec!["Vehicle", "Car"]);
    }

    /// A client attribute is kept in its own spelling when its expanded IRI matches a declared attribute IRI: under an aliased `@context` the forwarded term is the alias `spd`, not the IRI's last segment `speed`.
    #[tokio::test]
    async fn an_aliased_client_attribute_is_forwarded_under_its_alias_not_the_iri_segment() {
        let block = registration(&json!([{"entities": [{"type": ALIASED_VEHICLE}], "propertyNames": [ALIASED_SPEED]}]));
        let query = entity_query("type=Veh&attrs=spd");
        let forwarded = narrow_aliased(ClientQuery::EntityQuery(&query), &block, SplitEntities::Complete, aliased_context()).await;
        pretty_assertions::assert_eq!(forwarded.len(), 1);
        pretty_assertions::assert_eq!(forwarded[0].value_of(ForwardedParameter::Type), Some("Veh"));
        pretty_assertions::assert_eq!(forwarded[0].value_of(ForwardedParameter::Attrs), Some("spd"));
        assert!(
            !forwarded[0].to_query_string().contains("speed"),
            "the IRI's last segment must not leak: {}",
            forwarded[0].to_query_string()
        );
    }

    /// A client asking only for the type has the exclusive block's declared attribute added, compacted through the request context to the alias `spd` rather than the last segment `speed`.
    #[tokio::test]
    async fn an_aliased_block_attribute_is_supplied_under_its_alias_not_the_iri_segment() {
        let block = registration(&json!([{"entities": [{"type": ALIASED_VEHICLE}], "propertyNames": [ALIASED_SPEED]}]));
        let query = entity_query("type=Veh");
        let forwarded = narrow_aliased(ClientQuery::EntityQuery(&query), &block, SplitEntities::Complete, aliased_context()).await;
        pretty_assertions::assert_eq!(forwarded.len(), 1);
        pretty_assertions::assert_eq!(forwarded[0].value_of(ForwardedParameter::Type), Some("Veh"));
        pretty_assertions::assert_eq!(forwarded[0].value_of(ForwardedParameter::Attrs), Some("spd"));
        assert!(
            !forwarded[0].to_query_string().contains("speed"),
            "the IRI's last segment must not leak: {}",
            forwarded[0].to_query_string()
        );
    }

    /// Clause 4.3.6.6: a registration configuring its own `jsonldContext` is
    /// sent every name in that context (the client's type, attributes, query
    /// and filter terms reworded, and the block's supplied names compacted
    /// against it), never in the client's spelling.
    #[tokio::test]
    async fn a_configured_context_rewords_every_forwarded_name() {
        let engine = offline_engine();
        let inbound = ContextSpec::from_body(aliased_context());
        let forwarding = ContextSpec::from_body(
            JsonLiteral::admit(&json!({ "velocity": ALIASED_SPEED, "Car": ALIASED_VEHICLE }).to_string()).expect("an admissible literal"),
        );
        let resolver = TermResolver::new(&engine, &inbound);
        let names = ForwardedNames::prepare(&engine, &resolver, &forwarding, Rewording::Reword)
            .await
            .expect("the forwarding context processes");

        let block = registration(&json!([{"entities": [{"type": ALIASED_VEHICLE}], "propertyNames": [ALIASED_SPEED]}]));
        let query = entity_query("type=Veh&attrs=spd&q=spd%3E1&jsonKeys=spd&csf=spd%3D%3D1");
        let forwarded = ForwardedQuery::for_registration(
            ClientQuery::EntityQuery(&query),
            &block,
            SplitEntities::Complete,
            None,
            TemporalAnswer::AsRequested,
            &names,
        )
        .await
        .expect("narrowing succeeds");
        pretty_assertions::assert_eq!(forwarded.len(), 1);
        pretty_assertions::assert_eq!(forwarded[0].value_of(ForwardedParameter::Type), Some("Car"));
        pretty_assertions::assert_eq!(forwarded[0].value_of(ForwardedParameter::Attrs), Some("velocity"));
        pretty_assertions::assert_eq!(forwarded[0].value_of(ForwardedParameter::Q), Some("velocity>1"));
        pretty_assertions::assert_eq!(forwarded[0].value_of(ForwardedParameter::JsonKeys), Some("velocity"));
        pretty_assertions::assert_eq!(forwarded[0].value_of(ForwardedParameter::Csf), Some("velocity==1"));

        let supplied = ForwardedQuery::for_registration(
            ClientQuery::EntityQuery(&entity_query("")),
            &block,
            SplitEntities::Complete,
            None,
            TemporalAnswer::AsRequested,
            &names,
        )
        .await
        .expect("narrowing succeeds");
        pretty_assertions::assert_eq!(supplied[0].value_of(ForwardedParameter::Type), Some("Car"));
        pretty_assertions::assert_eq!(supplied[0].value_of(ForwardedParameter::Attrs), Some("velocity"));
    }
}
