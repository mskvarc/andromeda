//! Applying a clause 4.5.23 join to a rendered temporal payload, behind the
//! `temporal_linked_retrieval` extension.
//!
//! All three temporal read endpoints -- `GET /temporal/entities/{id}`,
//! `GET /temporal/entities` and `POST /temporal/entityOperations/query` -- run
//! the Linked Entity engine the same way once they hold their rendered roots:
//! the temporal service traverses over the request's window and the engine
//! plans the payload, which the temporal writer spells. None of the handlers
//! decides anything about Linked Entities itself.

use crate::history_read::TemporalHistoryRead;
use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
use andromeda_http_response::linked_resolution_warning::with_linked_resolution_warnings;
use andromeda_linked_retrieval::edge::LinkedEdge;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_render::{
    response::ResponsePayload,
    temporal::join_writer::{JoinedShape, write_temporal_join},
};
use andromeda_temporal::error::TemporalError;
use serde_json::Value;

/// What applying a clause 4.5.23 join to the rendered roots produced.
pub(crate) struct AppliedTemporalJoin {
    /// The response warnings, with one appended per Linked Entity clause
    /// 6.3.17 leaves unresolved.
    pub(crate) warnings: Vec<NgsiLdWarning>,
    /// The identifiers of the Linked Entities the payload carries, in
    /// first-encounter order. Clause 5.5.14 asks an `EntityMap` created
    /// alongside a joined query to include them.
    pub(crate) linked_ids: Vec<Identifier>,
    /// Where the join put the Linked Entities, or `None` when the request
    /// asked for no join that retrieves.
    pub(crate) shape: Option<JoinedShape>,
}

/// Resolve the request's join and write it into `entities`.
///
/// `read` is the history the roots came out of: the traversal reaches the
/// Linked Entities through the same service, inside the same target, over the
/// same temporal window and under the same JSON-LD context.
///
/// `seed` is what the locally read roots named, collected while they were
/// rendered and before any clause 4.5.8 or 4.5.19 projection erased the
/// `object` members; without it a join over `temporalValues` would silently
/// find nothing.
///
/// The returned warnings are the ones passed in plus one per Linked Entity the
/// traversal could not reach: clause 6.3.17 makes the `NGSILD-Warning` header
/// the only way a client learns the body is incomplete, since clause 4.5.23
/// leaves an unresolvable Relationship as a plain URI rather than failing.
///
/// # Errors
///
/// Returns [`TemporalError`] when the window or the projection cannot be
/// compiled, or the base entity store cannot be read.
pub(crate) async fn apply_temporal_join(
    read: &TemporalHistoryRead<'_>,
    entities: &mut Vec<Value>,
    seed: &[LinkedEdge],
    warnings: Vec<NgsiLdWarning>,
) -> Result<AppliedTemporalJoin, TemporalError> {
    let Some(joined) = read
        .service
        .join_linked_temporal(read.target, entities, seed, read.params, read.context)
        .await?
    else {
        return Ok(AppliedTemporalJoin {
            warnings,
            linked_ids: Vec::new(),
            shape: None,
        });
    };
    let plan = joined.plan();
    let linked_ids = plan.linked_ids().into_iter().cloned().collect(); // clone: the Entity Map outlives the plan
    let shape = write_temporal_join(&plan, entities);
    Ok(AppliedTemporalJoin {
        warnings: with_linked_resolution_warnings(warnings, joined.failures()),
        linked_ids,
        shape: Some(shape),
    })
}

/// The payload a joined single-entity retrieval answers with.
///
/// Clause 4.5.23: a flat join puts the root and the entities it links to side
/// by side, which turns a single-entity retrieval into a collection. An inline
/// join nests them into the root, leaving one entity.
///
/// `None` says the joined roots were empty, which an inline join cannot
/// produce -- it rewrites its root in place and never removes it -- so the
/// caller treats it as the broken invariant it is rather than rendering
/// something.
pub(crate) fn single_retrieval_payload(shape: JoinedShape, joined: Vec<Value>) -> Option<ResponsePayload> {
    match shape {
        JoinedShape::Appended => Some(ResponsePayload::Collection(joined)),
        JoinedShape::Nested => joined.into_iter().next().map(ResponsePayload::Single),
    }
}

#[cfg(test)]
mod tests {
    use crate::join::single_retrieval_payload;
    use andromeda_ngsi_ld_render::{response::ResponsePayload, temporal::join_writer::JoinedShape};
    use serde_json::{Value, json};

    fn entity(id: &str) -> Value {
        json!({"id": id, "type": "Thing", "reading": [{"type": "Property", "value": 1, "observedAt": "2024-01-01T00:00:00Z"}]})
    }

    fn identifiers(entities: &[Value]) -> Vec<&str> {
        entities.iter().filter_map(|entity| entity["id"].as_str()).collect()
    }

    /// Clause 4.5.23: a flat join turns a single-entity retrieval into a
    /// collection, with the root first.
    #[test]
    fn a_flat_join_turns_a_single_retrieval_into_a_root_first_collection() {
        let joined = vec![entity("urn:ngsi-ld:T:A"), entity("urn:ngsi-ld:T:B")];

        let payload = single_retrieval_payload(JoinedShape::Appended, joined).expect("a payload");

        match payload {
            ResponsePayload::Collection(entities) => {
                pretty_assertions::assert_eq!(identifiers(&entities), vec!["urn:ngsi-ld:T:A", "urn:ngsi-ld:T:B"]);
            }
            single @ (ResponsePayload::Single(_) | ResponsePayload::Batch(_)) => {
                panic!("a flat join answers with a collection, not {single:?}")
            }
        }
    }

    /// An inline join leaves the retrieval a single entity.
    #[test]
    fn an_inline_join_leaves_a_single_retrieval_single() {
        let payload = single_retrieval_payload(JoinedShape::Nested, vec![entity("urn:ngsi-ld:T:A")]).expect("a payload");

        match payload {
            ResponsePayload::Single(single) => pretty_assertions::assert_eq!(single["id"], "urn:ngsi-ld:T:A"),
            many @ (ResponsePayload::Collection(_) | ResponsePayload::Batch(_)) => {
                panic!("an inline join answers with one entity, not {many:?}")
            }
        }
    }

    /// An inline join over no root at all is a broken invariant rather than an
    /// empty body, and says so.
    #[test]
    fn an_inline_join_over_no_root_reports_the_broken_invariant() {
        assert!(single_retrieval_payload(JoinedShape::Nested, Vec::new()).is_none());
    }
}
