//! The `EntityMap` a temporal query runs under (clauses 5.5.14 and 5.14.5).
//!
//! Resolving what the query runs under happens before any context source is
//! contacted, since the map fixes which of them are contacted at all; recording
//! what they contributed, and shedding the entities the prepared page found no
//! longer matching, happens once they have answered.

use crate::{
    error::TemporalHttpClassify,
    history_read::TemporalHistoryRead,
    temporal::{ProblemResponse, ResultInstanceExt, TemporalQueryParameters, TimeRel, local_map_entities::LocalHistoryEntities},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity_map::{
    contribution::SourceContribution,
    fingerprint::{FingerprintedRequest, fingerprint_of},
    recording::RecordingFailure,
    reuse::{EntityMapFlag, EntityMapUse},
    service::EntityMapService,
};
use andromeda_http_headers::ngsild_entity_map::NgsiLdEntityMap;
use andromeda_ngsi_ld_lexicon::uri::Uri as NgsiLdUri;
use andromeda_ngsi_ld_model::entity_map::EntityMap;
use andromeda_ngsi_ld_request::query::entity_selector::EntitySelectorParams;
use andromeda_temporal::error::TemporalError;
use axum::http::Uri;
use chrono::{DateTime, Utc};

/// Resolve the Entity Map the query runs under and hold the query to it.
///
/// Clause 5.5.14: a referenced live map fixes the entities the query considers
/// and the sources it contacts, and holds the query to the creating request's
/// filters (the temporal window excepted); an expired or unknown one yields no
/// inference, so a fresh map is created instead. The fingerprint is taken
/// before the query is narrowed. A map-scoped follow-up may omit the temporal
/// window, which then stands for all history up to a far-future horizon.
///
/// # Errors
///
/// The classified failure of fingerprinting the request or of reading the map
/// store, and [`TemporalError::Validation`] when the far-future horizon cannot
/// be parsed.
pub(crate) async fn resolve_query_map(
    entity_maps: &EntityMapService,
    target: &ContextTarget,
    params: &mut TemporalQueryParameters,
    referenced: Option<&NgsiLdEntityMap>,
    instance: &Uri,
) -> Result<EntityMapUse, ProblemResponse> {
    if !target.permits_federation() {
        return Ok(EntityMapUse::None);
    }
    let fingerprint = fingerprint_of(FingerprintedRequest::QueryTemporal(params)).with_instance(instance)?;
    let map_use = entity_maps
        .resolve_use(
            referenced.map(NgsiLdEntityMap::as_inner),
            EntityMapFlag::from_parameter(params.entity_map),
            &fingerprint,
            params.entity_map_lifetime.as_ref(),
        )
        .await
        .with_instance(instance)?;
    if let Some(map) = map_use.reused() {
        hold_selection_to_map(map, &mut params.id, &mut params.entity_selectors);
        if params.timerel.is_none() && params.time_at.is_none() {
            params.timerel = Some(TimeRel::Before);
            params.time_at = Some(
                DateTime::parse_from_rfc3339("9999-12-31T23:59:59Z")
                    .map(|instant| instant.with_timezone(&Utc))
                    .map_err(|source| TemporalError::validation(format!("Invalid entity-map temporal horizon: {source}")))
                    .with_instance(instance)?,
            );
        }
    }
    Ok(map_use)
}

/// Hold the query's entity selection to the entities the reused map lists.
///
/// Clause 5.5.14: "If a subsequent request references an existent `EntityMap`, it
/// shall be used for the purposes of Entity registration matching, and queries
/// shall be filtered to only consider the Entities listed [...] An `EntityMap`
/// fixes the Entities to be considered for subsequent requests based on it."
///
/// A query string says what it selects through the flat identifier list; a
/// clause 5.2.23 body says it through the union of clause 5.2.33 selectors,
/// which is what compilation reads whenever the body carried one. Both have to
/// be narrowed, or the POST binding would answer with entities the map does not
/// list. A selector left with no identifier admits nothing and so contributes
/// nothing to the union; a union emptied that way leaves the request
/// considering no entity at all, which only the flat list can express.
fn hold_selection_to_map(map: &EntityMap, id: &mut Option<Vec<NgsiLdUri>>, selectors: &mut Vec<EntitySelectorParams>) {
    if selectors.is_empty() {
        *id = Some(EntityMapService::scope_query_ids(map, id.as_deref()));
        return;
    }
    selectors.retain_mut(|selector| {
        let scoped = EntityMapService::scope_query_ids(map, selector.id.as_deref());
        let admits_an_entity = !scoped.is_empty();
        selector.id = Some(scoped);
        admits_an_entity
    });
    *id = match selectors.as_slice() {
        // The flat list summarises a single selector, exactly as the clause
        // 5.2.23 body conversion writes it.
        [selector] => selector.id.clone(), // clone: the summary owns its copy of the selector's identifiers
        // Several selectors have no single flat summary, and the union decides.
        [_, _, ..] => None,
        // Every selector was emptied, so the union admits nothing.
        [] => Some(Vec::new()),
    };
}

/// Record the Entity Map the query ran under and return its location.
///
/// The recording itself is the same for every read that runs under a map and
/// belongs to the map policy; this is the temporal query's half of it: the
/// histories this broker still answers with, together with the Linked Entities
/// the join reached, which clause 5.5.14 counts as part of the same unitary
/// request, and the classification of the two vocabularies of failure.
///
/// # Errors
///
/// The classified failure of listing the matching identifiers or of writing to
/// the map store.
pub(crate) async fn record_query_map(
    entity_maps: &EntityMapService,
    read: &TemporalHistoryRead<'_>,
    map_use: EntityMapUse,
    contributions: Vec<SourceContribution>,
    linked_ids: Vec<NgsiLdUri>,
    instance: &Uri,
) -> Result<Option<NgsiLdUri>, ProblemResponse> {
    let local = LocalHistoryEntities::Queried {
        read,
        linked: &linked_ids,
        instance,
    };
    entity_maps
        .record_read(map_use, contributions, &local, local.page_filtering())
        .await
        .map_err(|failure| match failure {
            RecordingFailure::LocalEntities(problem) => problem,
            RecordingFailure::Store(error) => error.classify_for_temporal_http().with_instance(instance.clone()), // clone: the problem response owns the request URI it names
        })
}

#[cfg(test)]
mod tests {
    use crate::temporal::query::request_entity_map::hold_selection_to_map;
    use andromeda_ngsi_ld_lexicon::uri::Uri as NgsiLdUri;
    use andromeda_ngsi_ld_model::entity_map::EntityMap;
    use andromeda_ngsi_ld_request::query::entity_selector::EntitySelectorParams;
    use chrono::{TimeDelta, Utc};

    /// An entity identifier, as the request boundary would have validated it.
    fn identifier(raw: &str) -> NgsiLdUri {
        raw.parse().expect("a valid NGSI-LD URI")
    }

    /// A live map over the entities this broker holds itself.
    fn map_over(entity_ids: &[&str]) -> EntityMap {
        EntityMap::local(
            identifier("urn:ngsi-ld:EntityMap:scoping"),
            Utc::now() + TimeDelta::hours(1),
            entity_ids.iter().map(|id| identifier(id)),
        )
    }

    /// One clause 5.2.33 selector over the given identifiers.
    fn selector(entity_ids: &[&str]) -> EntitySelectorParams {
        EntitySelectorParams {
            id: Some(entity_ids.iter().map(|id| identifier(id)).collect()),
            ..EntitySelectorParams::default()
        }
    }

    /// The identifiers a selector was narrowed to, as wire text.
    fn selector_ids(selectors: &[EntitySelectorParams]) -> Vec<Vec<&str>> {
        selectors
            .iter()
            .map(|selector| selector.id.as_deref().unwrap_or_default().iter().map(NgsiLdUri::as_str).collect())
            .collect()
    }

    /// Clause 5.5.14: a query string names its entities in the flat identifier
    /// list, and a follow-up naming none considers everything the map lists.
    #[test]
    fn a_query_string_follow_up_naming_no_identifier_considers_the_whole_map() {
        let map = map_over(&["urn:ngsi-ld:Vehicle:a", "urn:ngsi-ld:Vehicle:b"]);
        let mut id = None;
        let mut selectors = Vec::new();

        hold_selection_to_map(&map, &mut id, &mut selectors);

        pretty_assertions::assert_eq!(
            id.as_deref().unwrap_or_default().iter().map(NgsiLdUri::as_str).collect::<Vec<_>>(),
            vec!["urn:ngsi-ld:Vehicle:a", "urn:ngsi-ld:Vehicle:b"]
        );
    }

    /// Clause 5.5.14: "queries shall be filtered to only consider the Entities
    /// listed". A clause 5.2.33 selector is what compilation reads on the POST
    /// binding, so it is narrowed to the intersection with the map.
    #[test]
    fn a_body_selector_is_narrowed_to_the_entities_the_map_lists() {
        let map = map_over(&["urn:ngsi-ld:Vehicle:a", "urn:ngsi-ld:Vehicle:b"]);
        let mut id = None;
        let mut selectors = vec![selector(&["urn:ngsi-ld:Vehicle:b", "urn:ngsi-ld:Vehicle:outside"])];

        hold_selection_to_map(&map, &mut id, &mut selectors);

        pretty_assertions::assert_eq!(selector_ids(&selectors), vec![vec!["urn:ngsi-ld:Vehicle:b"]]);
        pretty_assertions::assert_eq!(
            id.as_deref().unwrap_or_default().iter().map(NgsiLdUri::as_str).collect::<Vec<_>>(),
            vec!["urn:ngsi-ld:Vehicle:b"]
        );
    }

    /// A body selector that named no identifier is held to the whole map, so
    /// the follow-up cannot widen past what the map fixed.
    #[test]
    fn a_body_selector_naming_no_identifier_is_held_to_the_whole_map() {
        let map = map_over(&["urn:ngsi-ld:Vehicle:a", "urn:ngsi-ld:Vehicle:b"]);
        let mut id = None;
        let mut selectors = vec![EntitySelectorParams::default()];

        hold_selection_to_map(&map, &mut id, &mut selectors);

        pretty_assertions::assert_eq!(selector_ids(&selectors), vec![vec!["urn:ngsi-ld:Vehicle:a", "urn:ngsi-ld:Vehicle:b"]]);
    }

    /// Clause 5.5.14: a follow-up naming only identifiers the map does not list
    /// considers no entity at all, which the emptied flat list carries.
    #[test]
    fn a_body_selector_naming_nothing_the_map_lists_leaves_no_entity_considered() {
        let map = map_over(&["urn:ngsi-ld:Vehicle:a"]);
        let mut id = None;
        let mut selectors = vec![selector(&["urn:ngsi-ld:Vehicle:outside"])];

        hold_selection_to_map(&map, &mut id, &mut selectors);

        assert!(selectors.is_empty(), "a selector admitting nothing contributes nothing to the union");
        pretty_assertions::assert_eq!(id, Some(Vec::new()));
    }

    /// A union keeps only the selectors that still admit an entity, and the
    /// remaining ones decide together.
    #[test]
    fn a_union_keeps_only_the_selectors_the_map_still_admits() {
        let map = map_over(&["urn:ngsi-ld:Vehicle:a", "urn:ngsi-ld:Vehicle:b"]);
        let mut id = None;
        let mut selectors = vec![
            selector(&["urn:ngsi-ld:Vehicle:outside"]),
            selector(&["urn:ngsi-ld:Vehicle:a"]),
            selector(&["urn:ngsi-ld:Vehicle:b"]),
        ];

        hold_selection_to_map(&map, &mut id, &mut selectors);

        pretty_assertions::assert_eq!(selector_ids(&selectors), vec![vec!["urn:ngsi-ld:Vehicle:a"], vec!["urn:ngsi-ld:Vehicle:b"]]);
        pretty_assertions::assert_eq!(id, None, "several selectors have no single flat summary");
    }
}
