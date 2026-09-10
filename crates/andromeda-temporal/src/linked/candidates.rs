//! Which Linked Entities a rendered temporal representation names.
//!
//! A temporal representation carries one attribute instance per observation, so
//! an attribute whose Relationship pointed somewhere different at each instant
//! names several Linked Entities. Reading them out is a pure walk over rendered
//! JSON — it never touches history, a store or a context — which is why it sits
//! apart from the traversal in [`crate::linked`] that consumes it.

use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_render::relationship_targets::rendered_relationship_targets;
use indexmap::IndexSet;
use serde_json::Value;
use std::slice;

/// The members of a rendered temporal entity that never name a Linked Entity.
///
/// Everything else is an attribute, whose instances the walk inspects.
const STRUCTURAL_MEMBERS: [&str; 6] = ["id", "type", "@context", "createdAt", "modifiedAt", "deletedAt"];

/// Whether a candidate may be resolved outside this broker's own history.
///
/// Clause 4.5.23.1 gates it on the `objectType` annotation: only a Relationship
/// that carries one declares its object to be an Internal Linked Entity that
/// some other context source may hold. An unannotated object that this broker
/// has no history for stays a plain URI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteEligibility {
    /// A referencing Relationship carried an `objectType`.
    Eligible,
    /// No referencing Relationship carried one.
    LocalOnly,
}

/// One traversal candidate: the id a Relationship names, and whether the
/// remainder of the system may be asked for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemporalLinkedCandidate {
    /// The linked entity's identifier.
    pub id: Uri,
    /// Whether clause 4.5.23.1 admits a non-local resolution of it.
    pub eligibility: RemoteEligibility,
    /// Every attribute whose Relationship named this identifier, in
    /// first-encounter order, each appearing once.
    ///
    /// Read off the rendered representation's key at the same point
    /// [`rendered_relationship_targets`] reads the object it points at -- before any format
    /// projection (clause 4.5.5, 4.5.19) collapses the attribute's instances
    /// and erases the association. This is what lets a clause 4.21 nested
    /// `pick=rel{a}` / `omit=rel{a}` sub-projection be matched to the right
    /// identifier without re-deriving it from a possibly-projected rendering
    /// later.
    ///
    /// Two different Relationships can name the same target -- `relA{a}` and
    /// a bare `relB` both pointing at the same id, say -- and clause 4.21's
    /// union rule has to see every one of them to decide whether the target
    /// renders narrowed or in full, so every naming attribute is kept, not
    /// only the first. A key that could not be parsed as an attribute name
    /// contributes nothing to this list, which does not stop the identifier
    /// itself from being a candidate -- only the nested-projection narrowing
    /// loses that one attribute's association.
    pub attributes: Vec<AttributeName>,
}

/// The identifier a rendered temporal entity carries, when it carries a valid
/// one.
pub(crate) fn identifier_of(entity: &Value) -> Option<Uri> {
    entity.get("id").and_then(Value::as_str).and_then(|raw| raw.parse().ok())
}

/// Collect, in first-encounter order, every not-yet-visited identifier the
/// given rendered temporal entities name through a Relationship.
///
/// Every instance of every attribute is inspected, because a temporal
/// representation carries one instance per observation and a Relationship may
/// have pointed somewhere different at each of them. `objectList` order is
/// preserved, and an id named several times appears once, remotely eligible if
/// any of its referencing instances carried an `objectType`.
pub fn collect_candidates<'a>(entities: impl IntoIterator<Item = &'a Value>, visited: &IndexSet<Uri>) -> Vec<TemporalLinkedCandidate> {
    let mut seen: IndexSet<Uri> = IndexSet::new();
    let mut candidates: Vec<TemporalLinkedCandidate> = Vec::new();
    for entity in entities {
        let Some(members) = entity.as_object() else {
            continue;
        };
        for (name, member) in members {
            if STRUCTURAL_MEMBERS.contains(&name.as_str()) {
                continue;
            }
            // Parsed once per attribute rather than per instance: every
            // instance of one attribute shares the same key.
            let attribute = name.parse::<AttributeName>().ok();
            for instance in instances_of(member) {
                collect_from_instance(instance, attribute.as_ref(), visited, &mut seen, &mut candidates);
            }
        }
    }
    candidates
}

/// The attribute instances one rendered member holds.
///
/// A temporal member is normally an array of instances, but a single-instance
/// attribute may have been normalised to a bare object, and both shapes carry
/// the same Relationships.
fn instances_of(member: &Value) -> impl Iterator<Item = &Value> {
    match member {
        Value::Array(instances) => instances.iter(),
        single @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_)) => slice::from_ref(single).iter(),
    }
}

/// Record the identifiers one rendered attribute instance points at.
///
/// `seen` and `candidates` are index-aligned: `seen`'s position of an id is the
/// position of its [`TemporalLinkedCandidate`], which is how a repeated id
/// upgrades an earlier local-only entry to a remotely eligible one and
/// accumulates a second naming attribute onto its `attributes` list rather
/// than replacing the first.
fn collect_from_instance(
    instance: &Value,
    attribute: Option<&AttributeName>,
    visited: &IndexSet<Uri>,
    seen: &mut IndexSet<Uri>,
    candidates: &mut Vec<TemporalLinkedCandidate>,
) {
    let Some(fields) = instance.as_object() else {
        return;
    };
    let eligibility = if fields.contains_key("objectType") {
        RemoteEligibility::Eligible
    } else {
        RemoteEligibility::LocalOnly
    };

    let objects = fields.get("object").into_iter().chain(fields.get("objectList"));
    for object in objects {
        for id in rendered_relationship_targets(object) {
            if visited.contains(&id) {
                continue;
            }
            if let Some(index) = seen.get_index_of(&id) {
                if let Some(candidate) = candidates.get_mut(index) {
                    if eligibility == RemoteEligibility::Eligible {
                        candidate.eligibility = RemoteEligibility::Eligible;
                    }
                    if let Some(attribute) = attribute
                        && !candidate.attributes.iter().any(|existing| existing == attribute)
                    {
                        candidate.attributes.push(attribute.clone()); // clone: the candidate owns its attribute association
                    }
                }
            } else {
                seen.insert(id.clone()); // clone: the duplicate guard owns the ids it records
                candidates.push(TemporalLinkedCandidate {
                    id,
                    eligibility,
                    attributes: attribute.cloned().into_iter().collect(), // clone: the candidate owns its attribute association
                });
            }
        }
    }
}
