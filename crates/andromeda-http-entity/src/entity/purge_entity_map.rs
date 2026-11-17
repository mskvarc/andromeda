//! The `EntityMap` a purge runs under (Table 6.4.3.3-2).
//!
//! Table 6.4.3.3-2 makes `NGSILD-EntityMap` a request header every
//! implementation "shall support" on `DELETE /entities`: "If present, the
//! EntityMap supplied is used for determining the set of Entities requested
//! during the purge operation. The location of the EntityMap used in the purge
//! operation is returned in the response."
//!
//! A purge only ever reuses the map it is handed, and never creates one. Three
//! readings agree on that. Clause 5.5.14 enumerates the operations that may
//! occur as part of a distributed transactional request (Retrieve Entity,
//! Query Entities, Retrieve Temporal Evolution of an Entity, Query Temporal
//! Evolution of Entities), and Purge Entities is not among them, so a purge is
//! never the request a map is created for. Table 6.4.3.3-3 admits only 204, 207
//! and 400, never the "201 Created (in case an `EntityMap` has been
//! (re)created)" that Table 6.4.3.2-3 gives Query Entities, so a purge has no
//! way to report a creation. And Table 6.4.3.3-2 itself says "the location of
//! the `EntityMap` **used** in the purge operation", which is not a map the
//! purge would have created for a later request. Table 6.4.3.3-1 listing
//! neither `entityMap` nor `entityMapLifetime` corroborates it without
//! deciding it, since that table is not exhaustive either: it omits `attrs`
//! and `local`, both of which clause 5.6.21.4 requires.
//!
//! A reference that expired or cannot be accessed therefore "yields no
//! inference" (clause 5.5.14) and leaves the purge scoped by its own query
//! alone, answering with no map location because it used no map.

use crate::{error::EntityResultExt, state::EntityRouteServices};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity_map::{fingerprint::purge_fingerprint_of, query_scoping::hold_selection_to_map, service::ReferencedEntityMap};
use andromeda_federation::request::EntityMapInUse;
use andromeda_http_headers::ngsild_entity_map::NgsiLdEntityMap;
use andromeda_http_response::{headers::with_entity_map, problem_response::ProblemResponse};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_model::entity_map::EntityMap;
use andromeda_ngsi_ld_query::filter::reading::filter_reading::FilterReading;
use andromeda_ngsi_ld_request::query::{entity::admitted::AdmittedEntityQuery, selection_mut::SelectionMut};
use axum::{http::Uri as HttpUri, response::Response};

/// Which `EntityMap` a purge determined its entities from (Table 6.4.3.3-2).
pub(crate) enum PurgeEntityMap {
    /// The live map the request supplied: it fixed the entities purged, its
    /// registrations fix the sources the purge is forwarded to (clause 5.5.14),
    /// and its location is returned in the response. Boxed because an
    /// [`EntityMap`] dwarfs the empty variant.
    Used(Box<EntityMap>),
    /// No map determined the entity set: the request supplied none, or supplied
    /// one that has expired or cannot be accessed, from which clause 5.5.14
    /// allows no inference. There is no location to return.
    None,
}

impl PurgeEntityMap {
    /// Answer with the location of the map the purge used, when it used one.
    pub(crate) fn attach_to(&self, response: Response) -> Response {
        match self {
            PurgeEntityMap::Used(map) => with_entity_map(response, &map.id),
            PurgeEntityMap::None => response,
        }
    }

    /// The map in use for forwarding: a referenced map fixes the registrations
    /// the purge contacts and the location each is told (clause 5.5.14).
    pub(crate) fn in_use(&self) -> EntityMapInUse<'_> {
        match self {
            PurgeEntityMap::Used(map) => EntityMapInUse::Referenced(map),
            PurgeEntityMap::None => EntityMapInUse::None,
        }
    }
}

/// Resolve the map the purge determines its entities from, and hold the query
/// to it.
///
/// The fingerprint is taken over the query compiled as written before it is
/// narrowed, with the context source filter `csf`, so a purge under a map is
/// compared against the request as written, and over nothing else: clause
/// 5.5.14 does not name Purge Entities among the requests under a map, so the
/// other inputs of the request that created the map are not the purge's. Clause 5.6.21.4 has the
/// purge "run a Query Entities operation (clause 5.6.2) to retrieve a list of
/// ids of Entities for deletion" (the cross-reference is the spec's own, and
/// misnumbered: Query Entities is clause 5.7.2). The request the map fixes is
/// therefore that Query Entities, and it is fingerprinted as one: a map minted
/// by `GET /entities?type=Vehicle&entityMap=true` is the map a purge over the
/// same filters reuses, and a purge over different filters is the changed
/// request clause 5.5.14 forbids.
///
/// A snapshot branch contacts no context source and so runs under no map
/// (clause 5.5.15).
///
/// # Errors
///
/// Returns a [`ProblemResponse`] when the request cannot be fingerprinted, when
/// the map store cannot be read, or when the purge changed a filter parameter
/// of the request that created the map it references.
pub(crate) async fn resolve_purge_map(
    state: &EntityRouteServices,
    target: &ContextTarget,
    query: &mut AdmittedEntityQuery,
    context: &ContextSpec,
    csf: Option<&FilterReading>,
    referenced: Option<&NgsiLdEntityMap>,
    instance: &HttpUri,
) -> Result<PurgeEntityMap, ProblemResponse> {
    if !target.permits_federation() {
        return Ok(PurgeEntityMap::None);
    }
    let Some(referenced) = referenced else {
        return Ok(PurgeEntityMap::None);
    };
    let written = state
        .entity_query
        .compile_query_cached(query, context, state.entity_query.language_targeting())
        .await
        .with_entity_instance(instance)?;
    let fingerprint = purge_fingerprint_of(&written.selection, csf).with_entity_instance(instance)?;
    let resolved = state
        .entity_maps
        .resolve_reuse(referenced.as_inner(), &fingerprint)
        .await
        .with_entity_instance(instance)?;
    match resolved {
        ReferencedEntityMap::Live(map) => {
            let SelectionMut { id, entity_selectors } = query.selection_mut();
            hold_selection_to_map(&map, id, entity_selectors);
            // `resolve_reuse` already hands back a boxed map.
            Ok(PurgeEntityMap::Used(map))
        }
        ReferencedEntityMap::Unavailable => Ok(PurgeEntityMap::None),
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::purge_entity_map::PurgeEntityMap;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::entity_map::EntityMap;
    use axum::{
        http::StatusCode,
        response::{IntoResponse, Response},
    };
    use chrono::{TimeZone, Utc};

    /// The 204 a purge answers with when nothing failed.
    fn no_content() -> Response {
        StatusCode::NO_CONTENT.into_response()
    }

    /// The location the response advertises, if it advertises one.
    fn advertised_map(response: &Response) -> Option<&str> {
        response.headers().get("ngsild-entitymap").and_then(|value| value.to_str().ok())
    }

    /// A live map fixed to one locally held entity, standing in for a resolved
    /// referenced map.
    fn map_used() -> EntityMap {
        let identifier: Identifier = "urn:ngsi-ld:EntityMap:used".parse().expect("valid URI");
        let expires_at = Utc.with_ymd_and_hms(2030, 1, 1, 0, 0, 0).single().expect("valid instant");
        EntityMap::local(identifier, expires_at, [])
    }

    /// Table 6.4.3.3-2: "The location of the `EntityMap` used in the purge
    /// operation is returned in the response."
    #[test]
    fn a_purge_that_used_a_map_answers_with_its_location() {
        let response = PurgeEntityMap::Used(Box::new(map_used())).attach_to(no_content());
        pretty_assertions::assert_eq!(advertised_map(&response), Some("urn:ngsi-ld:EntityMap:used"));
    }

    /// A purge that supplied no map, or one clause 5.5.14 allows no inference
    /// from, used none: there is no location to return, and inventing one would
    /// tell the client its entity set was fixed when it was not.
    #[test]
    fn a_purge_that_used_no_map_answers_with_no_location() {
        let response = PurgeEntityMap::None.attach_to(no_content());
        pretty_assertions::assert_eq!(advertised_map(&response), None);
        pretty_assertions::assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }
}
