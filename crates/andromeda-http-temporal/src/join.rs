//! Applying a clause 4.5.23 join to a rendered temporal payload.
//!
//! All three temporal read endpoints — `GET /temporal/entities/{id}`,
//! `GET /temporal/entities` and `POST /temporal/entityOperations/query` — do
//! the same thing once they hold their rendered roots: traverse to the Linked
//! Entities over the same window, then either append them beside the roots or
//! nest them inside. Neither the shape of the traversal nor the shape of the
//! two renderings differs between the endpoints, so it is spelled once here.

use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
use andromeda_http_response::linked_resolution_warning::with_linked_resolution_warnings;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_render::{response::ResponsePayload, temporal::inline_join::apply_temporal_inline_join};
use andromeda_ngsi_ld_request::{
    join_traversal::{RetrievingJoin, traversal_bounds},
    query::temporal::TemporalQueryParameters,
};
use andromeda_temporal::{
    error::TemporalError,
    linked::{ResolvedLinkedTemporalEntities, candidates::TemporalLinkedCandidate},
    service::TemporalService,
};
use serde_json::Value;

/// What applying a clause 4.5.23 join to the rendered roots produced.
pub(crate) struct AppliedTemporalJoin {
    /// The response warnings, with one appended per Linked Entity clause
    /// 6.3.17 leaves unresolved.
    pub(crate) warnings: Vec<NgsiLdWarning>,
    /// The identifiers of the Linked Entities the traversal resolved, in
    /// traversal order. Spec 5.5.14 asks an EntityMap created alongside a
    /// joined query to include them.
    pub(crate) linked_ids: Vec<Uri>,
}

/// Resolve the request's join and fold it into `entities`.
///
/// `seed` is what the roots named, collected while they were rendered and
/// before any clause 4.5.5 or 4.5.19 projection erased the `object` members;
/// without it a join over `temporalValues` would silently find nothing.
///
/// `entities` are the rendered roots, and the join rewrites them in place: for
/// [`RetrievingJoin::Flat`] the Linked Entities are appended after the roots,
/// in traversal order (clause 6.4.3.2); for [`RetrievingJoin::Inline`] each
/// root has them nested under the Relationship instances that name them.
///
/// The returned warnings are the ones passed in plus one per Linked Entity the
/// traversal could not reach: clause 6.3.17 makes the `NGSILD-Warning` header
/// the only way a client learns the body is incomplete, since clause 4.5.23
/// leaves an unresolvable Relationship as a plain URI rather than failing.
///
/// # Errors
///
/// Returns [`TemporalError`] when the query cannot be compiled or the base
/// entity store cannot be read.
pub(crate) async fn apply_temporal_join(
    service: &TemporalService,
    join: RetrievingJoin,
    entities: &mut Vec<Value>,
    seed: &[TemporalLinkedCandidate],
    params: &TemporalQueryParameters,
    context: &ContextSpec,
    warnings: Vec<NgsiLdWarning>,
) -> Result<AppliedTemporalJoin, TemporalError> {
    let (join_level, contained_by) = traversal_bounds(params.join_level, params.contained_by.as_deref());

    let resolved = service
        .resolve_linked_temporal(entities, seed, params, join_level, &contained_by, context)
        .await?;
    let linked_ids: Vec<Uri> = resolved.entities.keys().cloned().collect();

    Ok(AppliedTemporalJoin {
        warnings: fold_resolution(join, entities, resolved, warnings),
        linked_ids,
    })
}

/// Fold a finished traversal into the rendered roots.
///
/// Separate from the traversal because it is the whole of the rendering
/// decision and none of the resolution: a flat join appends, an inline join
/// nests, and either way the unresolved Linked Entities become warnings.
fn fold_resolution(
    join: RetrievingJoin,
    entities: &mut Vec<Value>,
    resolved: ResolvedLinkedTemporalEntities,
    warnings: Vec<NgsiLdWarning>,
) -> Vec<NgsiLdWarning> {
    let ResolvedLinkedTemporalEntities { entities: linked, failures } = resolved;
    match join {
        // Clause 6.4.3.2: the roots keep their place and the linked entities
        // follow them, in traversal order.
        RetrievingJoin::Flat => entities.extend(linked.into_values()),
        RetrievingJoin::Inline => {
            for root in entities.iter_mut() {
                apply_temporal_inline_join(root, &linked);
            }
        }
    }
    with_linked_resolution_warnings(warnings, &failures)
}

/// The payload a joined single-entity retrieval answers with.
///
/// Clause 4.5.23: a flat join puts the root and the entities it links to side
/// by side, which turns a single-entity retrieval into a collection. An inline
/// join nests them into the root, leaving one entity.
///
/// `None` says the joined roots were empty, which an inline join cannot
/// produce — it rewrites its root in place and never removes it — so the caller
/// treats it as the broken invariant it is rather than rendering something.
pub(crate) fn single_retrieval_payload(join: RetrievingJoin, joined: Vec<Value>) -> Option<ResponsePayload> {
    match join {
        RetrievingJoin::Flat => Some(ResponsePayload::Collection(joined)),
        RetrievingJoin::Inline => joined.into_iter().next().map(ResponsePayload::Single),
    }
}

#[cfg(test)]
mod tests {
    use crate::join::{fold_resolution, single_retrieval_payload};
    use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
    use andromeda_ngsi_ld_render::response::ResponsePayload;
    use andromeda_ngsi_ld_representation::linked_resolution_failure::{LinkedResolutionFailure, LinkedResolutionFailureKind};
    use andromeda_ngsi_ld_request::join_traversal::RetrievingJoin;
    use andromeda_temporal::linked::ResolvedLinkedTemporalEntities;
    use serde_json::{Value, json};

    fn entity(id: &str) -> Value {
        json!({"id": id, "type": "Thing", "reading": [{"type": "Property", "value": 1, "observedAt": "2024-01-01T00:00:00Z"}]})
    }

    fn resolution(linked: &[&str]) -> ResolvedLinkedTemporalEntities {
        let mut resolved = ResolvedLinkedTemporalEntities::default();
        for id in linked {
            resolved.entities.insert(id.parse().expect("valid URI"), entity(id));
        }
        resolved
    }

    fn identifiers(entities: &[Value]) -> Vec<&str> {
        entities.iter().filter_map(|entity| entity["id"].as_str()).collect()
    }

    /// Clause 6.4.3.2: a flat join keeps the roots where they are and appends
    /// the linked entities after them, in traversal order.
    #[test]
    fn a_flat_join_appends_the_linked_entities_after_the_roots() {
        let mut entities = vec![entity("urn:ngsi-ld:T:A")];

        fold_resolution(
            RetrievingJoin::Flat,
            &mut entities,
            resolution(&["urn:ngsi-ld:T:C", "urn:ngsi-ld:T:B"]),
            Vec::new(),
        );

        pretty_assertions::assert_eq!(identifiers(&entities), vec!["urn:ngsi-ld:T:A", "urn:ngsi-ld:T:C", "urn:ngsi-ld:T:B"]);
    }

    /// An inline join nests instead of appending, so the collection still holds
    /// exactly the roots it started with.
    #[test]
    fn an_inline_join_nests_without_appending() {
        let mut entities = vec![json!({
            "id": "urn:ngsi-ld:T:A",
            "type": "Thing",
            "parkedAt": [{"type": "Relationship", "object": "urn:ngsi-ld:T:B", "observedAt": "2024-01-01T00:00:00Z"}]
        })];

        fold_resolution(RetrievingJoin::Inline, &mut entities, resolution(&["urn:ngsi-ld:T:B"]), Vec::new());

        pretty_assertions::assert_eq!(identifiers(&entities), vec!["urn:ngsi-ld:T:A"]);
        pretty_assertions::assert_eq!(entities[0]["parkedAt"][0]["entity"]["id"], "urn:ngsi-ld:T:B");
    }

    /// Clause 6.3.17: a Linked Entity the traversal could not reach becomes a
    /// response warning, and the warnings the forwarding already produced keep
    /// their place ahead of it.
    #[test]
    fn unresolved_linked_entities_become_warnings_after_the_existing_ones() {
        let existing = NgsiLdWarning::MiscellaneousWarning("Context source did not respond".to_owned());
        let mut resolved = resolution(&[]);
        resolved.failures.push(LinkedResolutionFailure {
            id: "urn:ngsi-ld:T:B".parse().expect("valid URI"),
            kind: LinkedResolutionFailureKind::Refused,
            detail: "the context source answered with a refusal".to_owned(),
        });
        let mut entities = vec![entity("urn:ngsi-ld:T:A")];

        let warnings = fold_resolution(RetrievingJoin::Flat, &mut entities, resolved, vec![existing.clone()]); // clone: the expectation keeps its own copy

        pretty_assertions::assert_eq!(warnings.len(), 2);
        pretty_assertions::assert_eq!(warnings.first(), Some(&existing));
        assert!(
            matches!(warnings.last(), Some(NgsiLdWarning::MiscellaneousPersistentWarning(detail)) if detail.contains("urn:ngsi-ld:T:B")),
            "the refusal became its clause 6.3.17 warning: {warnings:?}"
        );
    }

    /// A join that resolved everything adds no warning.
    #[test]
    fn a_complete_join_adds_no_warning() {
        let mut entities = vec![entity("urn:ngsi-ld:T:A")];

        let warnings = fold_resolution(RetrievingJoin::Flat, &mut entities, resolution(&["urn:ngsi-ld:T:B"]), Vec::new());

        assert!(warnings.is_empty());
    }

    /// Clause 4.5.23: a flat join turns a single-entity retrieval into a
    /// collection, with the root first.
    #[test]
    fn a_flat_join_turns_a_single_retrieval_into_a_root_first_collection() {
        let joined = vec![entity("urn:ngsi-ld:T:A"), entity("urn:ngsi-ld:T:B")];

        let payload = single_retrieval_payload(RetrievingJoin::Flat, joined).expect("a payload");

        match payload {
            ResponsePayload::Collection(entities) => {
                pretty_assertions::assert_eq!(identifiers(&entities), vec!["urn:ngsi-ld:T:A", "urn:ngsi-ld:T:B"]);
            }
            single @ ResponsePayload::Single(_) | single @ ResponsePayload::Batch(_) => {
                panic!("a flat join answers with a collection, not {single:?}")
            }
        }
    }

    /// An inline join leaves the retrieval a single entity.
    #[test]
    fn an_inline_join_leaves_a_single_retrieval_single() {
        let payload = single_retrieval_payload(RetrievingJoin::Inline, vec![entity("urn:ngsi-ld:T:A")]).expect("a payload");

        match payload {
            ResponsePayload::Single(single) => pretty_assertions::assert_eq!(single["id"], "urn:ngsi-ld:T:A"),
            many @ ResponsePayload::Collection(_) | many @ ResponsePayload::Batch(_) => {
                panic!("an inline join answers with one entity, not {many:?}")
            }
        }
    }

    /// An inline join over no root at all is a broken invariant rather than an
    /// empty body, and says so.
    #[test]
    fn an_inline_join_over_no_root_reports_the_broken_invariant() {
        assert!(single_retrieval_payload(RetrievingJoin::Inline, Vec::new()).is_none());
    }
}
