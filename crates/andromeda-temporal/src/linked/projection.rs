//! Applying the root request's clause 4.21 nested projection to the Linked
//! Entities a temporal join resolves (clause 4.5.23).
//!
//! `pick=rel{a}` keeps `rel` on the root -- settled by
//! `andromeda-ngsi-ld-render`'s `TemporalEntityRenderer` before any Linked
//! Entity is even resolved -- and narrows whatever `rel` links to down to `a`
//! plus its identity. This module derives which nested sub-projection applies
//! to which resolved identifier from the traversal's own
//! [`TemporalLinkedCandidate`]s rather than by re-walking the rendered roots:
//! a root rendered as `temporalValues` or `aggregatedValues` (clause 4.5.8,
//! 4.5.19) collapses an attribute's instances into a value array and erases
//! the `object` / `objectList` member a root-walk would need, exactly the
//! problem the traversal's own `seed` already solves for candidate discovery.
//! Each candidate already carries every attribute that named it, read off the
//! pre-transform normalized form, so reusing it here keeps the nested
//! narrowing correct under every representation.
//!
//! The union rule itself mirrors `andromeda-entity`'s
//! `crate::query::linked_projection` `Accumulator`: a target reached by two
//! Relationships, one nested and one bare, renders in full -- the bare
//! reference wins -- while two nested references union their terms.
//!
//! The narrowing itself is applied through
//! [`andromeda_ngsi_ld_render::temporal::linked_projection::narrow_linked_temporal_entity`].

use crate::{linked::candidates::TemporalLinkedCandidate, service::TemporalService};
use andromeda_jsonld::{context_spec::ContextSpec, term_expansion::ContextualExpander};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_query::{
    projection::{attribute_iri_index::AttributeIriIndex, compiled::Projection, matching::matching_term, term::ProjectionTerm},
    term_expander::TermExpander,
};
use andromeda_ngsi_ld_render::temporal::linked_projection::narrow_linked_temporal_entity;
use indexmap::{IndexMap, IndexSet};
use serde_json::Value;
use std::collections::HashMap;

/// Members of a rendered temporal entity that never carry an attribute key to
/// contribute to the [`AttributeIriIndex`].
const STRUCTURAL_MEMBERS: [&str; 3] = ["id", "type", "@context"];

impl TemporalService {
    /// Expand every distinct attribute key `root_candidates` and `linked`
    /// carry into an [`AttributeIriIndex`], so a nested projection can be
    /// matched against a compacted key without carrying a JSON-LD engine down
    /// into the walk.
    ///
    /// `root_candidates`' keys come from [`TemporalLinkedCandidate::attributes`]
    /// rather than from scanning a rendered root [`Value`], which is what
    /// keeps this index complete regardless of the root's representation
    /// (clause 4.5.8, 4.5.19). `linked`'s own keys are scanned directly: a
    /// resolved Linked Entity's top-level attribute keys survive every
    /// representation, only the shape of each attribute's instances differs.
    ///
    /// A key that fails to expand is simply left out of the index, the same
    /// "renders as unmatched" fallback [`AttributeIriIndex`] itself documents
    /// for a key it was never told about.
    pub(crate) async fn temporal_attribute_iri_index(
        &self,
        root_candidates: &[TemporalLinkedCandidate],
        linked: &IndexMap<Uri, Value>,
        context: &ContextSpec,
    ) -> AttributeIriIndex {
        let prepared = self.engine.prepare_term_context(context);
        let expander = ContextualExpander::new(&self.engine, &prepared);

        let mut keys: IndexSet<String> = root_candidates
            .iter()
            .flat_map(|candidate| candidate.attributes.iter())
            .map(AttributeName::to_string)
            .collect();
        for value in linked.values() {
            if let Some(map) = value.as_object() {
                keys.extend(map.keys().filter(|key| !STRUCTURAL_MEMBERS.contains(&key.as_str())).cloned()); // clone: the key set owns strings independent of the walked values
            }
        }

        let mut index = AttributeIriIndex::new();
        for key in keys {
            if let (Ok(name), Ok(iri)) = (key.parse::<AttributeName>(), expander.expand_term(&key).await) {
                index.insert(name, iri);
            }
        }
        index
    }
}

/// Narrow every resolved Linked Entity a nested `pick` / `omit` sub-projection
/// applies to.
///
/// A Linked Entity no candidate's attribute nested a projection into, or one
/// a bare-matching attribute reached alongside a nested one, is left exactly
/// as it rendered -- clause 4.21's union rule renders such a target in
/// full.
pub(crate) fn apply_nested_projections(
    root_candidates: &[TemporalLinkedCandidate],
    projection: &Projection,
    index: &AttributeIriIndex,
    linked: &mut IndexMap<Uri, Value>,
) {
    for (id, nested) in nested_projections(root_candidates, projection, index) {
        if let Some(entity) = linked.get_mut(&id) {
            narrow_linked_temporal_entity(entity, &nested, index);
        }
    }
}

/// The narrowing state accumulated for one identifier while every candidate's
/// naming attributes are walked.
///
/// `Full` absorbs every other contribution: once any naming attribute matches
/// a bare pick term (or omits nothing, per the omit rule below), the target
/// renders unrestricted regardless of what other attributes contributed,
/// which is why a later [`Accumulator::Terms`] merged into a
/// [`Accumulator::Full`] entry leaves it `Full`. Mirrors
/// `andromeda-entity`'s `query::linked_projection::Accumulator`.
#[derive(Debug, Clone)]
enum Accumulator {
    /// Render this linked entity with no projection restriction.
    Full,
    /// Render this linked entity under the union of these nested terms.
    Terms {
        /// Nested `pick` terms accumulated from every naming attribute.
        pick: Vec<ProjectionTerm>,
        /// Nested `omit` terms accumulated from every naming attribute.
        omit: Vec<ProjectionTerm>,
    },
}

/// Accumulate, from the first-level candidates' attribute associations, the
/// nested `pick` / `omit` sub-projection that applies to each identifier they
/// name.
///
/// Clause 4.21's union rule: a target two Relationships both name is
/// rendered one way, decided across every attribute that reaches it. A
/// Relationship whose matching `pick` term carries no nested sub-projection
/// (a bare reference) forces the target to render in full, absorbing any
/// nested terms another Relationship contributed -- and does so regardless of
/// which order the attributes are walked in, since [`merge_full`]
/// unconditionally overwrites and [`merge_pick`] / [`merge_omit`] both leave
/// an existing `Full` entry untouched. A bare `omit` reference, by contrast,
/// says nothing about how the target it names renders (clause 4.21: it omits
/// the relationship from the *referencing* entity, not the target), so it
/// contributes no entry at all.
///
/// # Accepted approximation
///
/// Only first-level candidates are inspected: a nested term attached two or
/// more hops away from a root is not threaded further down, so a
/// second-level Linked Entity renders in full rather than narrowed. The
/// temporal join in [`crate::linked::resolve_linked_temporal`] resolves an
/// identifier once regardless of the depth it was reached at and does not
/// currently keep which level named it, and clause 4.21's own examples --
/// and this broker's join tests -- go one level deep. Extending the walk to
/// every level would mean threading that provenance through the traversal, a
/// larger change than this narrowing needs to be useful today.
fn nested_projections(root_candidates: &[TemporalLinkedCandidate], projection: &Projection, index: &AttributeIriIndex) -> HashMap<Uri, Projection> {
    if projection.pick.is_empty() && projection.omit.is_empty() {
        return HashMap::new();
    }

    let mut result: HashMap<Uri, Accumulator> = HashMap::new();
    for candidate in root_candidates {
        for attribute in &candidate.attributes {
            if !projection.pick.is_empty() {
                match matching_term(attribute.as_str(), &projection.pick, index) {
                    Some(term) if !term.linked_terms().is_empty() => merge_pick(&mut result, &candidate.id, term.linked_terms()),
                    // A bare pick term matched this attribute: the target
                    // renders in full, absorbing any narrowing another
                    // attribute contributed.
                    Some(_) => merge_full(&mut result, &candidate.id),
                    None => {}
                }
            }
            // Clause 4.21: `omit=rel{a}` omits `a` inside whatever `rel`
            // links to, not `rel` itself, so only a nested omit contributes
            // an entry here.
            if !projection.omit.is_empty()
                && let Some(term) = matching_term(attribute.as_str(), &projection.omit, index)
                && !term.linked_terms().is_empty()
            {
                merge_omit(&mut result, &candidate.id, term.linked_terms());
            }
        }
    }

    result
        .into_iter()
        .filter_map(|(id, accumulator)| match accumulator {
            Accumulator::Full => None,
            Accumulator::Terms { pick, omit } => Some((id, Projection { pick, omit })),
        })
        .collect()
}

/// Merge a nested `pick` sub-projection into `id`'s accumulated entry,
/// leaving a `Full` entry untouched.
fn merge_pick(result: &mut HashMap<Uri, Accumulator>, id: &Uri, nested: &[ProjectionTerm]) {
    match result.entry(id.clone()).or_insert_with(|| Accumulator::Terms {
        pick: Vec::new(),
        omit: Vec::new(),
    }) {
        Accumulator::Full => {}
        Accumulator::Terms { pick, .. } => pick.extend(nested.iter().cloned()), // clone: owned terms attached to the accumulated per-id projection
    }
}

/// Merge a nested `omit` sub-projection into `id`'s accumulated entry,
/// leaving a `Full` entry untouched.
fn merge_omit(result: &mut HashMap<Uri, Accumulator>, id: &Uri, nested: &[ProjectionTerm]) {
    match result.entry(id.clone()).or_insert_with(|| Accumulator::Terms {
        pick: Vec::new(),
        omit: Vec::new(),
    }) {
        Accumulator::Full => {}
        Accumulator::Terms { omit, .. } => omit.extend(nested.iter().cloned()), // clone: owned terms attached to the accumulated per-id projection
    }
}

/// Widen `id`'s accumulated entry to `Full`: it renders unrestricted, which
/// absorbs whatever nested terms other attributes contributed, and cannot be
/// narrowed back down by anything merged in afterward.
fn merge_full(result: &mut HashMap<Uri, Accumulator>, id: &Uri) {
    result.insert(id.clone(), Accumulator::Full); // clone: the accumulator owns its own key independent of the candidate
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::linked::candidates::RemoteEligibility;
    use iri_rs::IriBuf;
    use serde_json::json;
    use std::slice;

    fn uri(value: &str) -> Uri {
        value.parse().expect("valid URI")
    }

    fn iri(name: &str) -> IriBuf {
        IriBuf::new(format!("https://example.com/{name}")).expect("valid IRI")
    }

    fn attribute_name(value: &str) -> AttributeName {
        value.parse().expect("valid attribute name")
    }

    fn index(keys: &[&str]) -> AttributeIriIndex {
        let mut index = AttributeIriIndex::new();
        for key in keys {
            index.insert(attribute_name(key), iri(key));
        }
        index
    }

    fn leaf(name: &str) -> ProjectionTerm {
        ProjectionTerm::Attribute {
            iri: iri(name),
            linked: Vec::new(),
        }
    }

    fn nested(name: &str, linked: Vec<ProjectionTerm>) -> ProjectionTerm {
        ProjectionTerm::Attribute { iri: iri(name), linked }
    }

    fn candidate(id: &str, attributes: &[&str]) -> TemporalLinkedCandidate {
        TemporalLinkedCandidate {
            id: uri(id),
            eligibility: RemoteEligibility::LocalOnly,
            attributes: attributes.iter().map(|name| attribute_name(name)).collect(),
        }
    }

    /// Clause 4.21: `pick=isParkedIn{category}` attaches `category` as the
    /// nested projection for whatever `isParkedIn` links to, read off the
    /// candidate's attribute association rather than a rendered root.
    #[test]
    fn a_nested_pick_term_attaches_to_the_identifier_its_relationship_names() {
        let candidates = [candidate("urn:ngsi-ld:Slot:1", &["isParkedIn"])];
        let projection = Projection {
            pick: vec![nested("isParkedIn", vec![leaf("category")])],
            omit: Vec::new(),
        };
        let index = index(&["isParkedIn", "category"]);

        let result = nested_projections(&candidates, &projection, &index);

        let nested = result.get(&uri("urn:ngsi-ld:Slot:1")).expect("nested projection recorded");
        pretty_assertions::assert_eq!(nested.pick, vec![leaf("category")]);
        assert!(nested.omit.is_empty());
    }

    /// A candidate with no recorded attribute association contributes no
    /// nested projection.
    #[test]
    fn a_candidate_without_an_attribute_association_contributes_nothing() {
        let candidates = [candidate("urn:ngsi-ld:Slot:1", &[])];
        let projection = Projection {
            pick: vec![nested("isParkedIn", vec![leaf("category")])],
            omit: Vec::new(),
        };
        let index = index(&["isParkedIn", "category"]);

        assert!(nested_projections(&candidates, &projection, &index).is_empty());
    }

    /// Clause 4.21's union rule: a target reached by a nested `relA{a}` and
    /// a bare `relB` -- both naming the same identifier -- renders in full.
    /// The bare reference contributes nothing to narrow, and forces the
    /// target to render unrestricted regardless of `relA`'s nested term.
    #[test]
    fn a_nested_term_and_a_bare_term_naming_the_same_target_render_it_in_full() {
        let candidates = [candidate("urn:ngsi-ld:Slot:1", &["relA", "relB"])];
        let projection = Projection {
            pick: vec![nested("relA", vec![leaf("a")]), leaf("relB")],
            omit: Vec::new(),
        };
        let index = index(&["relA", "relB", "a"]);

        assert!(
            !nested_projections(&candidates, &projection, &index).contains_key(&uri("urn:ngsi-ld:Slot:1")),
            "a bare-matched attribute must render the shared target in full"
        );
    }

    /// The same result holds regardless of which order the naming attributes
    /// are recorded in: `merge_full` unconditionally overwrites, so the bare
    /// reference wins whether it is walked before or after the nested one.
    #[test]
    fn the_full_render_outcome_is_independent_of_attribute_encounter_order() {
        let projection = Projection {
            pick: vec![nested("relA", vec![leaf("a")]), leaf("relB")],
            omit: Vec::new(),
        };
        let index = index(&["relA", "relB", "a"]);

        let nested_first = [candidate("urn:ngsi-ld:Slot:1", &["relA", "relB"])];
        let bare_first = [candidate("urn:ngsi-ld:Slot:1", &["relB", "relA"])];

        assert!(!nested_projections(&nested_first, &projection, &index).contains_key(&uri("urn:ngsi-ld:Slot:1")));
        assert!(!nested_projections(&bare_first, &projection, &index).contains_key(&uri("urn:ngsi-ld:Slot:1")));
    }

    /// Clause 4.21's union rule: a target reached by two nested references,
    /// `relA{a}` and `relB{b}`, is narrowed to the union of both -- `a` and
    /// `b` both survive.
    #[test]
    fn two_nested_terms_naming_the_same_target_union_their_terms() {
        let candidates = [candidate("urn:ngsi-ld:Slot:1", &["relA", "relB"])];
        let projection = Projection {
            pick: vec![nested("relA", vec![leaf("a")]), nested("relB", vec![leaf("b")])],
            omit: Vec::new(),
        };
        let index = index(&["relA", "relB", "a", "b"]);

        let result = nested_projections(&candidates, &projection, &index);

        let nested = result.get(&uri("urn:ngsi-ld:Slot:1")).expect("nested projection recorded");
        let mut pick = nested.pick.clone(); // clone: sorted for an order-independent comparison
        pick.sort_by(|left, right| term_iri(left).cmp(term_iri(right)));
        pretty_assertions::assert_eq!(pick, vec![leaf("a"), leaf("b")]);
    }

    /// The union's membership does not depend on which of the two nested
    /// attributes is recorded first.
    #[test]
    fn the_union_outcome_is_independent_of_attribute_encounter_order() {
        let projection = Projection {
            pick: vec![nested("relA", vec![leaf("a")]), nested("relB", vec![leaf("b")])],
            omit: Vec::new(),
        };
        let index = index(&["relA", "relB", "a", "b"]);

        let forward = [candidate("urn:ngsi-ld:Slot:1", &["relA", "relB"])];
        let backward = [candidate("urn:ngsi-ld:Slot:1", &["relB", "relA"])];

        for candidates in [&forward, &backward] {
            let result = nested_projections(candidates, &projection, &index);
            let nested = result.get(&uri("urn:ngsi-ld:Slot:1")).expect("nested projection recorded");
            assert!(nested.pick.contains(&leaf("a")));
            assert!(nested.pick.contains(&leaf("b")));
        }
    }

    /// The IRI a leaf [`ProjectionTerm::Attribute`] names, for sorting a
    /// union result into a deterministic comparison order.
    fn term_iri(term: &ProjectionTerm) -> &IriBuf {
        match term {
            ProjectionTerm::Attribute { iri, .. } => iri,
            ProjectionTerm::Id | ProjectionTerm::Type | ProjectionTerm::Scope => unreachable!("test terms are always attribute leaves"),
        }
    }

    /// Applying the accumulated nested projection reduces the resolved Linked
    /// Entity to the nested attribute plus its identity, leaving any other
    /// resolved entity untouched.
    #[test]
    fn applying_the_nested_projection_reduces_the_linked_entity() {
        let candidates = [candidate("urn:ngsi-ld:Slot:1", &["isParkedIn"])];
        let projection = Projection {
            pick: vec![nested("isParkedIn", vec![leaf("category")])],
            omit: Vec::new(),
        };
        let index = index(&["isParkedIn", "category", "name"]);
        let mut linked = IndexMap::new();
        linked.insert(
            uri("urn:ngsi-ld:Slot:1"),
            json!({
                "id": "urn:ngsi-ld:Slot:1",
                "type": "Slot",
                "category": [{"type": "Property", "value": "reserved", "observedAt": "2024-01-01T00:00:00Z"}],
                "name": [{"type": "Property", "value": "Bay 3", "observedAt": "2024-01-01T00:00:00Z"}]
            }),
        );

        apply_nested_projections(&candidates, &projection, &index, &mut linked);

        let slot = linked.get(&uri("urn:ngsi-ld:Slot:1")).expect("slot survives");
        assert_eq!(slot["id"], "urn:ngsi-ld:Slot:1");
        assert!(slot.get("category").is_some());
        assert!(slot.get("name").is_none());
    }

    /// The narrowing does not depend on the shape of the linked entity's own
    /// attribute instances: a `temporalValues`-rendered target -- each
    /// attribute collapsed to `{"values": [...]}`  -- is narrowed by its
    /// top-level keys exactly the same as a normalized one.
    #[test]
    fn applying_the_nested_projection_narrows_a_temporal_values_rendered_target() {
        let candidates = [candidate("urn:ngsi-ld:Slot:1", &["isParkedIn"])];
        let projection = Projection {
            pick: vec![nested("isParkedIn", vec![leaf("category")])],
            omit: Vec::new(),
        };
        let index = index(&["isParkedIn", "category", "name"]);
        let mut linked = IndexMap::new();
        linked.insert(
            uri("urn:ngsi-ld:Slot:1"),
            json!({
                "id": "urn:ngsi-ld:Slot:1",
                "type": "Slot",
                "category": {"type": "Property", "values": [["reserved", "2024-01-01T00:00:00Z"]]},
                "name": {"type": "Property", "values": [["Bay 3", "2024-01-01T00:00:00Z"]]}
            }),
        );

        apply_nested_projections(&candidates, &projection, &index, &mut linked);

        let slot = linked.get(&uri("urn:ngsi-ld:Slot:1")).expect("slot survives");
        assert!(slot.get("category").is_some());
        assert!(slot.get("name").is_none());
    }

    /// A Linked Entity no candidate's attribute nested a projection into is
    /// left exactly as it rendered.
    #[test]
    fn a_linked_entity_reached_without_nesting_is_left_untouched() {
        let candidates = [candidate("urn:ngsi-ld:Slot:1", &["isParkedIn"])];
        let projection = Projection {
            pick: vec![leaf("isParkedIn")],
            omit: Vec::new(),
        };
        let index = index(&["isParkedIn", "category"]);
        let mut linked = IndexMap::new();
        linked.insert(
            uri("urn:ngsi-ld:Slot:1"),
            json!({
                "id": "urn:ngsi-ld:Slot:1",
                "type": "Slot",
                "category": [{"type": "Property", "value": "reserved"}]
            }),
        );

        apply_nested_projections(&candidates, &projection, &index, &mut linked);

        let slot = linked.get(&uri("urn:ngsi-ld:Slot:1")).expect("slot survives");
        assert!(slot.get("category").is_some());
    }

    /// An empty projection attaches no nested projection to any identifier.
    #[test]
    fn an_empty_projection_attaches_nothing() {
        let candidates = [candidate("urn:ngsi-ld:Slot:1", &["isParkedIn"])];

        let result = nested_projections(&candidates, &Projection::default(), &index(&["isParkedIn"]));

        assert!(result.is_empty());
    }

    /// The [`slice::from_ref`] the module's callers use to hand a single
    /// candidate through the same `&[TemporalLinkedCandidate]` shape as many.
    #[test]
    fn a_single_candidate_slice_narrows_the_same_as_a_multi_element_one() {
        let single = candidate("urn:ngsi-ld:Slot:1", &["isParkedIn"]);
        let projection = Projection {
            pick: vec![nested("isParkedIn", vec![leaf("category")])],
            omit: Vec::new(),
        };
        let index = index(&["isParkedIn", "category"]);

        let result = nested_projections(slice::from_ref(&single), &projection, &index);

        assert!(result.contains_key(&uri("urn:ngsi-ld:Slot:1")));
    }
}
