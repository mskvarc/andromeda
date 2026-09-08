//! The clause 4.21 nested projection that applies to each linked entity:
//! deriving it from the `pick`/`omit` sub-projections its referencing
//! Relationships carry (clause 4.21), and applying it to the entity.

use crate::{error::EntityError, query::EntityQueryService};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::{attribute::instance::Attribute, entity::Entity};
use andromeda_ngsi_ld_query::projection::{compiled::Projection, term::ProjectionTerm};
use indexmap::IndexMap;
use iri_rs::IriBuf;
use std::collections::{HashMap, HashSet, VecDeque};

/// The projection accumulated for one linked entity, before it is turned
/// into the public `Option<Projection>` shape.
///
/// `Full` absorbs every other contribution: once any referencing
/// Relationship carries no nested projection, the entity renders in full
/// regardless of what other Relationships nested into it, which is why a
/// later [`Accumulator::Terms`] merged into a [`Accumulator::Full`] entry
/// leaves it `Full`.
#[derive(Debug, Clone)]
enum Accumulator {
    /// Render this linked entity with no projection restriction.
    Full,
    /// Render this linked entity under the union of these nested terms.
    Terms {
        /// Nested `pick` terms accumulated from every referencing Relationship.
        pick: Vec<ProjectionTerm>,
        /// Nested `omit` terms accumulated from every referencing Relationship.
        omit: Vec<ProjectionTerm>,
    },
}

/// One traversal level still to walk: the entities reached at that level,
/// and the pick/omit terms active while walking their attributes.
struct Level<'a> {
    /// The entities to walk at this level.
    entities: Vec<&'a Entity>,
    /// The nested `pick` terms active for this level's Relationships.
    pick: Vec<ProjectionTerm>,
    /// The nested `omit` terms active for this level's Relationships.
    omit: Vec<ProjectionTerm>,
}

impl EntityQueryService {
    /// Reduce every resolved linked entity to the nested projection that
    /// applies to it under the `join=flat` union rule (clause 4.21
    /// `LinkedEntityTerm`).
    ///
    /// `pick=rel{a,b}` reduces the entities reached through `rel` to `a`, `b`,
    /// `id` and `type`: the nested terms govern the linked entity's attribute
    /// members only, and its identity members always survive so a joined
    /// entity stays identifiable. A linked entity with no nested projection,
    /// or one reached by any Relationship that carried none, is left
    /// untouched.
    ///
    /// This is the flat rule and only the flat rule. `join=flat` returns each
    /// linked entity exactly once, so one narrowing has to serve every
    /// Relationship that reached it. `join=inline` nests a separate copy per
    /// Relationship and therefore narrows each copy separately, in the
    /// renderer — see [`crate::query::join_projection`].
    ///
    /// # Errors
    ///
    /// Returns [`EntityError`] when an attribute key cannot be expanded
    /// through the context.
    pub async fn apply_flat_linked_projections(
        &self,
        roots: &[Entity],
        linked: &mut IndexMap<Uri, Entity>,
        projection: &Projection,
        context: &ContextSpec,
    ) -> Result<(), EntityError> {
        let nested = self.linked_projections(roots, linked, projection, context).await?;
        for (id, nested) in nested {
            // An absent entry and an explicit `None` both mean "render in
            // full"; only a `Some` narrows the entity.
            let Some(nested) = nested else {
                continue;
            };
            if let Some(entity) = linked.get_mut(&id) {
                self.project_entity(entity, &nested, context).await?;
            }
        }
        Ok(())
    }

    /// Union of the nested projections that apply to each linked entity,
    /// keyed by id.
    ///
    /// An id absent from the map carries the same meaning as an explicit
    /// `None` entry: no referencing Relationship, at any traversal level,
    /// attached a nested projection to it, so it renders in full. `Some`
    /// carries the union of every nested `pick`/`omit` sub-projection
    /// (clause 4.21 `{...}`) attached to that id by any Relationship or
    /// `ListRelationship`, at any traversal level, that named it with a nested
    /// projection.
    ///
    /// Attribute keys are matched through the request's `@context`, the same
    /// way [`crate::query::attribute_projection::apply_projection`] matches
    /// the root entities' own attributes -- `roots` and `linked` hold
    /// compacted entities, keyed by short term or compact IRI, while a
    /// [`ProjectionTerm::Attribute`] always carries the fully expanded IRI.
    ///
    /// # Accepted approximation
    ///
    /// The walk visits each id once. When a second, differently nested path
    /// reaches an id already visited, its terms are unioned into that id's own
    /// entry but are not carried further down into the id's targets — the
    /// deeper half of that second path contributes nothing. Making it exact
    /// would mean keying the walk by path rather than by id, which is
    /// unbounded on a cyclic graph, and the flat representation renders each
    /// linked entity once regardless, so the union already collapses those
    /// paths at the level below. The exact per-path semantics exist where they
    /// are observable: `join=inline`, where the renderer walks each
    /// Relationship separately.
    ///
    /// # Errors
    ///
    /// Returns [`EntityError`] when an attribute key cannot be expanded
    /// through the context.
    pub(crate) async fn linked_projections(
        &self,
        roots: &[Entity],
        linked: &IndexMap<Uri, Entity>,
        projection: &Projection,
        context: &ContextSpec,
    ) -> Result<HashMap<Uri, Option<Projection>>, EntityError> {
        if projection.pick.is_empty() && projection.omit.is_empty() {
            return Ok(HashMap::new());
        }

        let mut result: HashMap<Uri, Accumulator> = HashMap::new();
        let mut visited: HashSet<Uri> = HashSet::new();
        let mut queue: VecDeque<Level> = VecDeque::new();
        queue.push_back(Level {
            entities: roots.iter().collect(),
            pick: projection.pick.clone(), // clone: owned terms carried through the level queue
            omit: projection.omit.clone(), // clone: owned terms carried through the level queue
        });

        while let Some(Level {
            entities,
            pick: active_pick,
            omit: active_omit,
        }) = queue.pop_front()
        {
            // The nested pick/omit terms this level attached to each id, so
            // the next level knows which entity to descend into and under
            // which projection.
            let mut next_level: HashMap<Uri, (Vec<ProjectionTerm>, Vec<ProjectionTerm>)> = HashMap::new();

            for entity in &entities {
                for (name, value) in &entity.attrs {
                    for attr in value.instances() {
                        let Some(target_ids) = relationship_targets(attr) else {
                            continue;
                        };
                        let expanded = self.expanded_key(name.as_str(), context).await?;

                        if !active_pick.is_empty() {
                            match term_for_attribute(name.as_str(), &expanded, &active_pick) {
                                Some(term) if !term.linked_terms().is_empty() => {
                                    for id in &target_ids {
                                        merge_pick(&mut result, id, term.linked_terms());
                                        next_level.entry(id.clone()).or_default().0.extend(term.linked_terms().iter().cloned()); // clone: owned terms carried into the next level
                                    }
                                }
                                Some(_) => {
                                    for id in &target_ids {
                                        merge_full(&mut result, id);
                                        next_level.remove(id);
                                    }
                                }
                                None => {}
                            }
                        }

                        // Clause 4.21: a bare `omit` term (no nested
                        // `{...}`) only drops this attribute from the entity
                        // being rendered here -- it says nothing about how
                        // the linked entity itself renders. It therefore
                        // contributes no entry: that target's rendering is
                        // governed by whichever other Relationship reaches
                        // it, or renders in full if none does.
                        if !active_omit.is_empty()
                            && let Some(term) = term_for_attribute(name.as_str(), &expanded, &active_omit)
                            && !term.linked_terms().is_empty()
                        {
                            for id in &target_ids {
                                merge_omit(&mut result, id, term.linked_terms());
                                next_level.entry(id.clone()).or_default().1.extend(term.linked_terms().iter().cloned()); // clone: owned terms carried into the next level
                            }
                        }
                    }
                }
            }

            for (id, (pick, omit)) in next_level {
                if visited.contains(&id) {
                    continue;
                }
                // A later attribute in this same level may have widened the
                // id to `Full` after the nested contribution was recorded; a
                // `Full` target renders unrestricted and is not descended into.
                if matches!(result.get(&id), Some(Accumulator::Full)) {
                    continue;
                }
                if let Some(entity) = linked.get(&id) {
                    visited.insert(id.clone()); // clone: the visited set outlives this borrow of next_level
                    queue.push_back(Level {
                        entities: vec![entity],
                        pick,
                        omit,
                    });
                }
            }
        }

        Ok(result
            .into_iter()
            .map(|(id, accumulator)| {
                let projection = match accumulator {
                    Accumulator::Full => None,
                    Accumulator::Terms { pick, omit } => Some(Projection { pick, omit }),
                };
                (id, projection)
            })
            .collect())
    }
}

/// Merge a nested `pick` sub-projection into `id`'s accumulated entry,
/// leaving a `Full` entry untouched.
fn merge_pick(result: &mut HashMap<Uri, Accumulator>, id: &Uri, nested: &[ProjectionTerm]) {
    match result.entry(id.clone()).or_insert_with(|| Accumulator::Terms {
        pick: Vec::new(),
        omit: Vec::new(),
    }) {
        Accumulator::Full => {}
        Accumulator::Terms { pick, .. } => pick.extend(nested.iter().cloned()),
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
        Accumulator::Terms { omit, .. } => omit.extend(nested.iter().cloned()),
    }
}

/// Widen `id`'s accumulated entry to `Full`: it renders unrestricted, which
/// absorbs whatever nested terms other Relationships contributed.
fn merge_full(result: &mut HashMap<Uri, Accumulator>, id: &Uri) {
    result.insert(id.clone(), Accumulator::Full);
}

/// The `pick`/`omit` term naming the attribute keyed by `key`, whose expanded
/// IRI is `expanded`, if the term list names it.
///
/// Delegates to [`ProjectionTerm::matches_attribute`], which accepts either
/// the compact key or the expanded IRI -- the same match
/// [`crate::query::attribute_projection::apply_projection`] performs on the
/// root entities' own attributes.
fn term_for_attribute<'p>(key: &str, expanded: &IriBuf, terms: &'p [ProjectionTerm]) -> Option<&'p ProjectionTerm> {
    terms.iter().find(|term| term.matches_attribute(key, expanded))
}

/// The identifiers a Relationship or `ListRelationship` instance points at, or
/// `None` for every other attribute family.
fn relationship_targets(attr: &Attribute) -> Option<Vec<Uri>> {
    match attr {
        Attribute::Relationship(relationship) => Some(relationship.object.iter().cloned().collect()), // clone: owned ids returned to the caller
        Attribute::ListRelationship(list) => Some(list.object_list.iter().cloned().collect()),        // clone: owned ids returned to the caller
        Attribute::Property(_)
        | Attribute::GeoProperty(_)
        | Attribute::JsonProperty(_)
        | Attribute::LanguageProperty(_)
        | Attribute::ListProperty(_)
        | Attribute::VocabProperty(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::service_test_double::service;
    use serde_json::json;
    use std::slice::from_ref;

    fn entity(value: serde_json::Value) -> Entity {
        serde_json::from_value(value).expect("entity")
    }

    fn uri(value: &str) -> Uri {
        value.parse().expect("valid URI")
    }

    /// The expanded form of one of this module's test context terms -- a
    /// [`ProjectionTerm::Attribute`] always carries the fully expanded IRI,
    /// matching what [`context`] maps the same short `name` to.
    fn iri(name: &str) -> IriBuf {
        IriBuf::new(format!("https://example.com/{name}")).expect("valid IRI")
    }

    fn leaf(name: &str) -> ProjectionTerm {
        ProjectionTerm::Attribute {
            iri: iri(name),
            linked: Vec::new(),
        }
    }

    /// A context mapping every compact term this module's tests use to a
    /// deterministic, offline-resolvable expanded IRI -- no network fetch, so
    /// term expansion is exercised for real without flaky or slow I/O.
    fn context() -> ContextSpec {
        ContextSpec::none().with_body_context(Some(json!({
            "hasA": "https://example.com/hasA",
            "hasB": "https://example.com/hasB",
            "hasMany": "https://example.com/hasMany",
            "temperature": "https://example.com/temperature",
        })))
    }

    /// Clause 4.21: `hasA{temperature}` attaches `temperature` as the
    /// projection to apply when rendering whatever `hasA` links to. Entities
    /// are keyed by the compact term, proving attribute matching goes
    /// through the request context rather than comparing raw key text
    /// against the term's expanded IRI.
    #[tokio::test]
    async fn a_single_relationship_with_a_nested_pick_attaches_it_to_its_target() {
        let root = entity(json!({
            "id": "urn:ngsi-ld:Root:1",
            "type": "Root",
            "hasA": {"type": "Relationship", "object": "urn:ngsi-ld:Target:1"}
        }));
        let target = entity(json!({"id": "urn:ngsi-ld:Target:1", "type": "Target"}));
        let mut linked = IndexMap::new();
        linked.insert(uri("urn:ngsi-ld:Target:1"), target);
        let projection = Projection {
            pick: vec![ProjectionTerm::Attribute {
                iri: iri("hasA"),
                linked: vec![leaf("temperature")],
            }],
            omit: Vec::new(),
        };

        let result = service().linked_projections(&[root], &linked, &projection, &context()).await.expect("computes");

        let nested = result
            .get(&uri("urn:ngsi-ld:Target:1"))
            .expect("target has an entry")
            .as_ref()
            .expect("nested projection");
        assert_eq!(nested.pick, vec![leaf("temperature")]);
        assert!(nested.omit.is_empty(), "the nested projection omits nothing: {:?}", nested.omit);
    }

    /// The same relationship, keyed by its already-expanded IRI, matches
    /// too -- a compacted entity may carry either shape depending on which
    /// side of expansion the payload sits on.
    #[tokio::test]
    async fn a_relationship_keyed_by_its_expanded_iri_also_matches() {
        let root = entity(json!({
            "id": "urn:ngsi-ld:Root:1b",
            "type": "Root",
            "https://example.com/hasA": {"type": "Relationship", "object": "urn:ngsi-ld:Target:1b"}
        }));
        let target = entity(json!({"id": "urn:ngsi-ld:Target:1b", "type": "Target"}));
        let mut linked = IndexMap::new();
        linked.insert(uri("urn:ngsi-ld:Target:1b"), target);
        let projection = Projection {
            pick: vec![ProjectionTerm::Attribute {
                iri: iri("hasA"),
                linked: vec![leaf("temperature")],
            }],
            omit: Vec::new(),
        };

        let result = service().linked_projections(&[root], &linked, &projection, &context()).await.expect("computes");

        let nested = result
            .get(&uri("urn:ngsi-ld:Target:1b"))
            .expect("target has an entry")
            .as_ref()
            .expect("nested projection");
        assert_eq!(nested.pick, vec![leaf("temperature")]);
    }

    /// Clause 4.21: when two Relationships reach the same linked entity and
    /// only one of them carries a nested projection, the flat union rule
    /// renders that entity in full rather than narrowing it.
    #[tokio::test]
    async fn two_relationships_to_the_same_target_where_one_carries_no_nesting_render_in_full() {
        let root = entity(json!({
            "id": "urn:ngsi-ld:Root:2",
            "type": "Root",
            "hasA": {"type": "Relationship", "object": "urn:ngsi-ld:Target:2"},
            "hasB": {"type": "Relationship", "object": "urn:ngsi-ld:Target:2"}
        }));
        let target = entity(json!({"id": "urn:ngsi-ld:Target:2", "type": "Target"}));
        let mut linked = IndexMap::new();
        linked.insert(uri("urn:ngsi-ld:Target:2"), target);
        let projection = Projection {
            pick: vec![
                ProjectionTerm::Attribute {
                    iri: iri("hasA"),
                    linked: vec![leaf("temperature")],
                },
                ProjectionTerm::Attribute {
                    iri: iri("hasB"),
                    linked: Vec::new(),
                },
            ],
            omit: Vec::new(),
        };

        let result = service().linked_projections(&[root], &linked, &projection, &context()).await.expect("computes");

        assert_eq!(result.get(&uri("urn:ngsi-ld:Target:2")), Some(&None));
    }

    /// Clause 4.21 applies the same nested projection to every member of a
    /// `ListRelationship`'s `hasObjectList`.
    #[tokio::test]
    async fn a_list_relationship_attaches_its_nested_pick_to_every_target() {
        let root = entity(json!({
            "id": "urn:ngsi-ld:Root:3",
            "type": "Root",
            "hasMany": {"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Target:3a", "urn:ngsi-ld:Target:3b"]}
        }));
        let mut linked = IndexMap::new();
        linked.insert(uri("urn:ngsi-ld:Target:3a"), entity(json!({"id": "urn:ngsi-ld:Target:3a", "type": "Target"})));
        linked.insert(uri("urn:ngsi-ld:Target:3b"), entity(json!({"id": "urn:ngsi-ld:Target:3b", "type": "Target"})));
        let projection = Projection {
            pick: vec![ProjectionTerm::Attribute {
                iri: iri("hasMany"),
                linked: vec![leaf("temperature")],
            }],
            omit: Vec::new(),
        };

        let result = service().linked_projections(&[root], &linked, &projection, &context()).await.expect("computes");

        for id in ["urn:ngsi-ld:Target:3a", "urn:ngsi-ld:Target:3b"] {
            let nested = result.get(&uri(id)).expect("target has an entry").as_ref().expect("nested projection");
            assert_eq!(nested.pick, vec![leaf("temperature")]);
        }
    }

    /// Clause 4.21: a nested `omit` sub-projection propagates to the linked
    /// entity as an omit-projection, not a pick.
    #[tokio::test]
    async fn a_nested_omit_term_propagates_as_an_omit_projection() {
        let root = entity(json!({
            "id": "urn:ngsi-ld:Root:4",
            "type": "Root",
            "hasA": {"type": "Relationship", "object": "urn:ngsi-ld:Target:4"}
        }));
        let target = entity(json!({"id": "urn:ngsi-ld:Target:4", "type": "Target"}));
        let mut linked = IndexMap::new();
        linked.insert(uri("urn:ngsi-ld:Target:4"), target);
        let projection = Projection {
            pick: Vec::new(),
            omit: vec![ProjectionTerm::Attribute {
                iri: iri("hasA"),
                linked: vec![leaf("temperature")],
            }],
        };

        let result = service().linked_projections(&[root], &linked, &projection, &context()).await.expect("computes");

        let nested = result
            .get(&uri("urn:ngsi-ld:Target:4"))
            .expect("target has an entry")
            .as_ref()
            .expect("nested projection");
        assert!(nested.pick.is_empty(), "the nested projection picks nothing: {:?}", nested.pick);
        assert_eq!(nested.omit, vec![leaf("temperature")]);
    }

    /// Clause 4.21, decided invariant: a *bare* `omit` term (`omit=hasA`,
    /// no nested `{...}`) only removes `hasA` from the root's own rendering.
    /// It says nothing about how the entity `hasA` links to should render, so
    /// it contributes no entry -- that target is absent from the map, which
    /// carries the same "render in full" meaning as an explicit `None`.
    #[tokio::test]
    async fn a_bare_omit_term_with_no_nested_projection_contributes_no_entry() {
        let root = entity(json!({
            "id": "urn:ngsi-ld:Root:4b",
            "type": "Root",
            "hasA": {"type": "Relationship", "object": "urn:ngsi-ld:Target:4b"}
        }));
        let target = entity(json!({"id": "urn:ngsi-ld:Target:4b", "type": "Target"}));
        let mut linked = IndexMap::new();
        linked.insert(uri("urn:ngsi-ld:Target:4b"), target);
        let projection = Projection {
            pick: Vec::new(),
            omit: vec![ProjectionTerm::Attribute {
                iri: iri("hasA"),
                linked: Vec::new(),
            }],
        };

        let result = service().linked_projections(&[root], &linked, &projection, &context()).await.expect("computes");

        assert!(!result.contains_key(&uri("urn:ngsi-ld:Target:4b")));
    }

    /// Second-level nesting (`hasA{hasB{temperature}}`) attaches its
    /// innermost term to the entity `hasB` reaches from `hasA`'s target, one
    /// traversal level down from where `hasA{...}` itself attached.
    #[tokio::test]
    async fn a_second_level_nested_term_attaches_to_the_entity_it_is_reached_from() {
        let root = entity(json!({
            "id": "urn:ngsi-ld:Root:5",
            "type": "Root",
            "hasA": {"type": "Relationship", "object": "urn:ngsi-ld:Target:5a"}
        }));
        let level1 = entity(json!({
            "id": "urn:ngsi-ld:Target:5a",
            "type": "Target",
            "hasB": {"type": "Relationship", "object": "urn:ngsi-ld:Target:5b"}
        }));
        let level2 = entity(json!({"id": "urn:ngsi-ld:Target:5b", "type": "Target"}));
        let mut linked = IndexMap::new();
        linked.insert(uri("urn:ngsi-ld:Target:5a"), level1);
        linked.insert(uri("urn:ngsi-ld:Target:5b"), level2);
        let projection = Projection {
            pick: vec![ProjectionTerm::Attribute {
                iri: iri("hasA"),
                linked: vec![ProjectionTerm::Attribute {
                    iri: iri("hasB"),
                    linked: vec![leaf("temperature")],
                }],
            }],
            omit: Vec::new(),
        };

        let result = service().linked_projections(&[root], &linked, &projection, &context()).await.expect("computes");

        let level1_projection = result
            .get(&uri("urn:ngsi-ld:Target:5a"))
            .expect("level-1 target has an entry")
            .as_ref()
            .expect("nested projection");
        assert_eq!(
            level1_projection.pick,
            vec![ProjectionTerm::Attribute {
                iri: iri("hasB"),
                linked: vec![leaf("temperature")]
            }]
        );

        let level2_projection = result
            .get(&uri("urn:ngsi-ld:Target:5b"))
            .expect("level-2 target has an entry")
            .as_ref()
            .expect("nested projection");
        assert_eq!(level2_projection.pick, vec![leaf("temperature")]);
    }

    /// Clause 4.21: `hasA{temperature}` reduces the entity `hasA` links to
    /// down to `temperature` plus its identity members, which stay so the
    /// joined entity remains identifiable in the response.
    #[tokio::test]
    async fn applying_a_nested_pick_reduces_the_linked_entity_to_the_nested_attributes() {
        let root = entity(json!({
            "id": "urn:ngsi-ld:Root:7",
            "type": "Root",
            "hasA": {"type": "Relationship", "object": "urn:ngsi-ld:Target:7"}
        }));
        let mut linked = IndexMap::new();
        linked.insert(
            uri("urn:ngsi-ld:Target:7"),
            entity(json!({
                "id": "urn:ngsi-ld:Target:7",
                "type": "Target",
                "temperature": {"type": "Property", "value": 21},
                "hasB": {"type": "Relationship", "object": "urn:ngsi-ld:Target:7b"}
            })),
        );
        let projection = Projection {
            pick: vec![ProjectionTerm::Attribute {
                iri: iri("hasA"),
                linked: vec![leaf("temperature")],
            }],
            omit: Vec::new(),
        };

        service()
            .apply_flat_linked_projections(&[root], &mut linked, &projection, &context())
            .await
            .expect("applies");

        let target = linked.get(&uri("urn:ngsi-ld:Target:7")).expect("target survives");
        pretty_assertions::assert_eq!(target.id, uri("urn:ngsi-ld:Target:7"));
        assert!(target.attrs.contains_key("temperature"));
        assert!(!target.attrs.contains_key("hasB"));
    }

    /// Clause 4.21 union rule: a linked entity that any Relationship reached
    /// without a nested projection renders in full, so applying the nested
    /// projections leaves it untouched.
    #[tokio::test]
    async fn applying_leaves_a_target_reached_without_nesting_untouched() {
        let root = entity(json!({
            "id": "urn:ngsi-ld:Root:8",
            "type": "Root",
            "hasA": {"type": "Relationship", "object": "urn:ngsi-ld:Target:8"},
            "hasB": {"type": "Relationship", "object": "urn:ngsi-ld:Target:8"}
        }));
        let mut linked = IndexMap::new();
        linked.insert(
            uri("urn:ngsi-ld:Target:8"),
            entity(json!({
                "id": "urn:ngsi-ld:Target:8",
                "type": "Target",
                "temperature": {"type": "Property", "value": 21},
                "humidity": {"type": "Property", "value": 40}
            })),
        );
        let projection = Projection {
            pick: vec![
                ProjectionTerm::Attribute {
                    iri: iri("hasA"),
                    linked: vec![leaf("temperature")],
                },
                ProjectionTerm::Attribute {
                    iri: iri("hasB"),
                    linked: Vec::new(),
                },
            ],
            omit: Vec::new(),
        };

        service()
            .apply_flat_linked_projections(&[root], &mut linked, &projection, &context())
            .await
            .expect("applies");

        let target = linked.get(&uri("urn:ngsi-ld:Target:8")).expect("target survives");
        assert!(target.attrs.contains_key("temperature"));
        assert!(target.attrs.contains_key("humidity"));
    }

    /// Clause 4.21, exercised through the real ordering: the root projection
    /// runs inside the query, before the traversal. `omit=hasA{temperature}`
    /// omits `temperature` *inside* what `hasA` links to, so `hasA` itself has
    /// to survive the root projection -- were it removed there, no
    /// Relationship would be left for the nested projection to be derived
    /// from and the nested `omit` would silently do nothing.
    #[tokio::test]
    async fn a_nested_omit_survives_the_root_projection_and_reaches_the_linked_entity() {
        let service = service();
        let context = context();
        let projection = Projection {
            pick: Vec::new(),
            omit: vec![ProjectionTerm::Attribute {
                iri: iri("hasA"),
                linked: vec![leaf("temperature")],
            }],
        };

        let mut root = entity(json!({
            "id": "urn:ngsi-ld:Root:9",
            "type": "Root",
            "hasA": {"type": "Relationship", "object": "urn:ngsi-ld:Target:9"},
            "temperature": {"type": "Property", "value": 5}
        }));
        service.project_entity(&mut root, &projection, &context).await.expect("projects the root");
        assert!(root.attrs.contains_key("hasA"), "a nested omit term must not remove the relationship itself");
        assert!(root.attrs.contains_key("temperature"), "a nested omit term names no member of the root");

        let mut linked = IndexMap::new();
        linked.insert(
            uri("urn:ngsi-ld:Target:9"),
            entity(json!({
                "id": "urn:ngsi-ld:Target:9",
                "type": "Target",
                "temperature": {"type": "Property", "value": 21},
                "name": {"type": "Property", "value": "Depot"}
            })),
        );

        service
            .apply_flat_linked_projections(from_ref(&root), &mut linked, &projection, &context)
            .await
            .expect("applies");

        let target = linked.get(&uri("urn:ngsi-ld:Target:9")).expect("target survives");
        assert!(!target.attrs.contains_key("temperature"), "the nested omit reaches the linked entity");
        assert!(target.attrs.contains_key("name"));
    }

    /// Clause 4.21: a *bare* `omit` term still removes the attribute it
    /// names from the entity being projected -- only a term carrying a nested
    /// sub-projection defers to the linked entity.
    #[tokio::test]
    async fn a_bare_omit_term_removes_the_attribute_from_the_projected_entity() {
        let service = service();
        let projection = Projection {
            pick: Vec::new(),
            omit: vec![leaf("hasA")],
        };
        let mut root = entity(json!({
            "id": "urn:ngsi-ld:Root:10",
            "type": "Root",
            "hasA": {"type": "Relationship", "object": "urn:ngsi-ld:Target:10"},
            "temperature": {"type": "Property", "value": 5}
        }));

        service.project_entity(&mut root, &projection, &context()).await.expect("projects the root");

        assert!(!root.attrs.contains_key("hasA"));
        assert!(root.attrs.contains_key("temperature"));
    }

    /// The `join=flat` half of the shape contract the two join modes share.
    ///
    /// Clause 4.21 reduces a linked entity to the named attributes plus `id`
    /// and `type`; `scope` follows the clause 4.21 structural keyword rule
    /// and is dropped by a nested `pick` that does not name it. Every other
    /// entity member lives outside the attribute map -- the clause 4.8
    /// timestamps and the clause 4.7 geo-properties -- and neither mode may
    /// touch it, or the same linked entity would come back in two shapes.
    ///
    /// The inline counterpart is `andromeda-ngsi-ld-render`'s
    /// `a_nested_pick_preserves_every_entity_member_outside_the_attribute_map`
    /// and `a_nested_pick_drops_scope_unless_the_scope_keyword_is_nested_too`.
    /// The two must agree member for member.
    #[tokio::test]
    async fn the_flat_narrowing_leaves_the_same_linked_entity_shape_as_the_inline_one() {
        let root = entity(json!({
            "id": "urn:ngsi-ld:Root:11",
            "type": "Root",
            "hasA": {"type": "Relationship", "object": "urn:ngsi-ld:Target:11"}
        }));
        let mut linked = IndexMap::new();
        linked.insert(
            uri("urn:ngsi-ld:Target:11"),
            entity(json!({
                "id": "urn:ngsi-ld:Target:11",
                "type": "Target",
                "scope": "/Madrid",
                "createdAt": "2026-01-01T00:00:00Z",
                "modifiedAt": "2026-01-02T00:00:00Z",
                "deletedAt": "2026-01-03T00:00:00Z",
                "expiresAt": "2026-02-01T00:00:00Z",
                "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [0.0, 0.0]}},
                "observationSpace": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 1.0]}},
                "operationSpace": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [2.0, 2.0]}},
                "temperature": {"type": "Property", "value": 21},
                "humidity": {"type": "Property", "value": 40}
            })),
        );
        let projection = Projection {
            pick: vec![ProjectionTerm::Attribute {
                iri: iri("hasA"),
                linked: vec![leaf("temperature")],
            }],
            omit: Vec::new(),
        };

        service()
            .apply_flat_linked_projections(&[root], &mut linked, &projection, &context())
            .await
            .expect("applies");

        let target = linked.get(&uri("urn:ngsi-ld:Target:11")).expect("target survives");
        assert!(target.scope.is_none(), "a nested pick that does not name `scope` drops it");
        assert!(target.created_at.is_some());
        assert!(target.modified_at.is_some());
        assert!(target.deleted_at.is_some());
        assert!(target.expires_at.is_some());
        assert!(target.location.is_some());
        assert!(target.observation_space.is_some());
        assert!(target.operation_space.is_some());
        assert!(target.attrs.contains_key("temperature"));
        assert!(!target.attrs.contains_key("humidity"));
    }

    /// An empty `pick` and empty `omit` select every member with no
    /// restriction, so no linked entity gets a nested projection at all.
    #[tokio::test]
    async fn an_empty_projection_produces_no_entries() {
        let root = entity(json!({
            "id": "urn:ngsi-ld:Root:6",
            "type": "Root",
            "hasA": {"type": "Relationship", "object": "urn:ngsi-ld:Target:6"}
        }));
        let linked = IndexMap::new();
        let result = service()
            .linked_projections(&[root], &linked, &Projection::default(), &context())
            .await
            .expect("computes");
        assert!(result.is_empty());
    }
}
