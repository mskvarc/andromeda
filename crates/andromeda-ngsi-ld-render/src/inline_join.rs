//! The clause 4.5.23 `join=inline` expansion of a rendered entity.
//!
//! A request with `join=inline` asks for the entities a Relationship points at
//! to appear nested under the Relationship rather than being named by URI
//! alone. The expansion runs over an already-rendered value, not over the
//! typed entity, because the same expansion applies to a payload that reached
//! the broker as JSON-LD and was never a `Entity` at all.

use crate::{
    entity::EntityRenderer,
    error::RenderError,
    nested_projection::{LinkedProjection, NestedProjection},
};
use andromeda_http_headers::language::LanguageFilter;
use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, uri::Uri};
use andromeda_ngsi_ld_model::entity::Entity;
use indexmap::IndexMap;
use serde_json::{Map, Value};
use std::{collections::HashSet, str::FromStr};

/// What an inline expansion carries unchanged from the top of the walk to the
/// deepest nested entity.
struct InlineJoinContext<'a> {
    /// The entities the traversal resolved, keyed by identifier.
    linked: &'a IndexMap<Uri, Entity>,
    /// The `lang` filter every nested rendering negotiates against, if any.
    language_filter: Option<&'a LanguageFilter>,
}

/// Apply `join=inline` expansion to an already-rendered entity response.
///
/// Exposed within the crate (`pub(crate)`) because it applies to a `Value` as
/// readily as to an [`Entity`]: [`crate::snapshot::SnapshotRenderer`] runs it
/// over a payload that arrived already compacted, and [`EntityRenderer`] runs it
/// over one it just built.
///
/// `projection` carries the clause 4.21 nested sub-projections. They are
/// applied one Relationship instance at a time, as the walk descends, so two
/// Relationships that name the same target under different sub-projections
/// nest two differently narrowed copies of it. `None` narrows nothing.
///
/// The `linked` map legitimately contains entities that reference one another,
/// so the recursion is bounded by the set of ids already open on the current
/// path: a linked entity that references itself, or forms a cycle with another,
/// is rendered once and not descended into again. Without that guard a cyclic
/// linked entity recurses without end and overflows the stack from a request.
pub(crate) fn apply_inline_join(
    entity: &mut Value,
    linked: &IndexMap<Uri, Entity>,
    language_filter: Option<&LanguageFilter>,
    projection: Option<LinkedProjection<'_>>,
) -> Result<(), RenderError> {
    let context = InlineJoinContext { linked, language_filter };
    let nested = projection.map_or(NestedProjection::NONE, NestedProjection::root);
    let mut ancestors = HashSet::new();
    apply_inline_join_guarded(entity, &context, nested, &mut ancestors)
}

/// Expand one entity, tracking the ids already open on the current path so a
/// reference cycle terminates.
fn apply_inline_join_guarded(
    entity: &mut Value,
    context: &InlineJoinContext<'_>,
    nested: NestedProjection<'_>,
    ancestors: &mut HashSet<Uri>,
) -> Result<(), RenderError> {
    let self_id = entity.get("id").and_then(Value::as_str).and_then(|raw| raw.parse::<Uri>().ok());
    // Track this entity's id on the current path so a self-reference is caught.
    let seeded = self_id.as_ref().is_some_and(|id| ancestors.insert(id.clone()));

    let result = inline_join_object(entity, context, nested, ancestors);

    if seeded && let Some(id) = &self_id {
        ancestors.remove(id);
    }
    result
}

/// Walk every attribute instance of one rendered entity object.
///
/// The attribute key is what selects the sub-projection governing whatever that
/// attribute links to, which is why the walk iterates entries rather than bare
/// values.
fn inline_join_object(
    entity: &mut Value,
    context: &InlineJoinContext<'_>,
    nested: NestedProjection<'_>,
    ancestors: &mut HashSet<Uri>,
) -> Result<(), RenderError> {
    let Some(obj) = entity.as_object_mut() else { return Ok(()) };
    for (key, attr_val) in obj.iter_mut() {
        let target = nested.descend(key);
        match attr_val {
            Value::Object(attr_obj) => {
                inline_join_attribute_instance(attr_obj, context, target, ancestors)?;
            }
            Value::Array(items) => {
                for item in items.iter_mut() {
                    if let Value::Object(attr_obj) = item {
                        inline_join_attribute_instance(attr_obj, context, target, ancestors)?;
                    }
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
        }
    }
    Ok(())
}

/// Expand a single attribute instance with inline-joined linked entities.
///
/// For `Relationship` attributes, resolves the `object` URI against the linked
/// entity map and inserts the rendered entity under the `"entity"` key.
/// For `ListRelationship` attributes, resolves each URI in `objectList` and
/// inserts the rendered entities under the `"entityList"` key. `target` is the
/// sub-projection this attribute attached to whatever it links to.
fn inline_join_attribute_instance(
    attr_obj: &mut Map<String, Value>,
    context: &InlineJoinContext<'_>,
    target: NestedProjection<'_>,
    ancestors: &mut HashSet<Uri>,
) -> Result<(), RenderError> {
    let attr_type = attr_obj.get("type").and_then(Value::as_str).and_then(|raw| AttributeType::from_str(raw).ok());
    match attr_type {
        Some(AttributeType::Relationship) => {
            if let Some(object_uri) = attr_obj.get("object").and_then(Value::as_str).and_then(|raw| raw.parse::<Uri>().ok())
                && let Some(linked_entity) = context.linked.get(&object_uri)
            {
                let linked_val = render_linked(linked_entity, &object_uri, context, target, ancestors)?;
                attr_obj.insert("entity".to_string(), linked_val);
            }
        }
        Some(AttributeType::ListRelationship) => {
            if let Some(object_list) = attr_obj.get("objectList").and_then(Value::as_array) {
                // Collect the URIs first so the borrow of `attr_obj` is released
                // before the recursive expansion, which needs the mutable
                // ancestor set.
                let uris: Vec<Uri> = object_list.iter().filter_map(Value::as_str).filter_map(|raw| raw.parse::<Uri>().ok()).collect();
                let mut entity_list = Vec::new();
                for uri in &uris {
                    if let Some(linked_entity) = context.linked.get(uri) {
                        entity_list.push(render_linked(linked_entity, uri, context, target, ancestors)?);
                    }
                }
                if !entity_list.is_empty() {
                    attr_obj.insert("entityList".to_string(), Value::Array(entity_list));
                }
            }
        }
        // Only a Relationship or a ListRelationship names a linked entity; every
        // other attribute kind, and an unrecognised type, is left untouched.
        Some(
            AttributeType::Property
            | AttributeType::GeoProperty
            | AttributeType::VocabProperty
            | AttributeType::LanguageProperty
            | AttributeType::JsonProperty
            | AttributeType::ListProperty,
        )
        | None => {}
    }
    Ok(())
}

/// Render one linked entity, narrow it to the sub-projection the Relationship
/// that reached it carried, and expand it in turn — unless its id is already
/// open on the current path, in which case the back-reference is left
/// unexpanded so the cycle terminates.
///
/// The expansion descends before the narrowing so a Relationship the
/// sub-projection keeps still nests its own target: narrowing a member away
/// after its nested entity was built costs nothing, whereas narrowing first
/// would hide Relationships the deeper terms still name.
fn render_linked(
    linked_entity: &Entity,
    object_uri: &Uri,
    context: &InlineJoinContext<'_>,
    target: NestedProjection<'_>,
    ancestors: &mut HashSet<Uri>,
) -> Result<Value, RenderError> {
    let mut linked_val = EntityRenderer::new(linked_entity)
        .with_optional_language_filter(context.language_filter)
        .render()?;
    if !ancestors.contains(object_uri) {
        apply_inline_join_guarded(&mut linked_val, context, target, ancestors)?;
    }
    if !target.is_unrestricted() {
        target.narrow(&mut linked_val);
    }
    Ok(linked_val)
}

#[cfg(test)]
mod tests {
    use crate::{inline_join::apply_inline_join, nested_projection::LinkedProjection};
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
    use andromeda_ngsi_ld_model::entity::Entity;
    use andromeda_ngsi_ld_query::projection::{attribute_iri_index::AttributeIriIndex, compiled::Projection, term::ProjectionTerm};
    use indexmap::IndexMap;
    use iri_rs::IriBuf;
    use serde_json::{Value, json};

    fn parse_entity(value: Value) -> Entity {
        serde_json::from_value(value).expect("entity")
    }

    fn iri(name: &str) -> IriBuf {
        IriBuf::new(format!("https://example.com/{name}")).expect("valid IRI")
    }

    fn attribute_name(value: &str) -> AttributeName {
        value.parse().expect("valid attribute name")
    }

    /// An expansion index covering every key this module's projection tests
    /// name, mapping each to a deterministic offline IRI.
    fn index(keys: &[&str]) -> AttributeIriIndex {
        let mut index = AttributeIriIndex::new();
        for key in keys {
            index.insert(attribute_name(key), iri(key));
        }
        index
    }

    fn term(name: &str, linked: Vec<ProjectionTerm>) -> ProjectionTerm {
        ProjectionTerm::Attribute { iri: iri(name), linked }
    }

    /// Clause 4.21, per-instance semantics: two Relationships naming the same
    /// target under different sub-projections nest two differently narrowed
    /// copies of it — the union rule that governs `join=flat` would render both
    /// in full.
    #[test]
    fn inline_join_narrows_each_relationship_instance_by_its_own_sub_projection() {
        let target = parse_entity(json!({
            "id": "urn:t", "type": "T",
            "category": {"type": "Property", "value": "commercial"},
            "name": {"type": "Property", "value": "Depot"}
        }));
        let mut linked_map = IndexMap::new();
        linked_map.insert("urn:t".parse().expect("valid URI"), target);

        let projection = Projection {
            pick: vec![term("relA", vec![term("category", Vec::new())]), term("relB", Vec::new())],
            omit: Vec::new(),
        };
        let index = index(&["relA", "relB", "category", "name"]);

        let mut entity = json!({
            "id": "urn:a", "type": "A",
            "relA": {"type": "Relationship", "object": "urn:t"},
            "relB": {"type": "Relationship", "object": "urn:t"}
        });

        apply_inline_join(
            &mut entity,
            &linked_map,
            None,
            Some(LinkedProjection {
                projection: &projection,
                index: &index,
            }),
        )
        .expect("inline join");

        // Reached through the sub-projection: narrowed, identity intact.
        pretty_assertions::assert_eq!(entity["relA"]["entity"]["id"], "urn:t");
        assert!(entity["relA"]["entity"].get("category").is_some());
        assert!(entity["relA"]["entity"].get("name").is_none());
        // Reached without one: rendered in full.
        assert!(entity["relB"]["entity"].get("name").is_some());
    }

    /// Clause 4.21: a two-level nesting narrows the entity at each level by the
    /// terms that level carried.
    #[test]
    fn inline_join_applies_a_second_level_sub_projection_one_hop_further_down() {
        let inner = parse_entity(json!({
            "id": "urn:c", "type": "C",
            "zone": {"type": "Property", "value": "north"},
            "name": {"type": "Property", "value": "District"}
        }));
        let middle = parse_entity(json!({
            "id": "urn:b", "type": "B",
            "inner": {"type": "Relationship", "object": "urn:c"},
            "name": {"type": "Property", "value": "Building"}
        }));
        let mut linked_map = IndexMap::new();
        linked_map.insert("urn:b".parse().expect("valid URI"), middle);
        linked_map.insert("urn:c".parse().expect("valid URI"), inner);

        let projection = Projection {
            pick: vec![term("outer", vec![term("inner", vec![term("zone", Vec::new())])])],
            omit: Vec::new(),
        };
        let index = index(&["outer", "inner", "zone", "name"]);

        let mut entity = json!({
            "id": "urn:a", "type": "A",
            "outer": {"type": "Relationship", "object": "urn:b"}
        });

        apply_inline_join(
            &mut entity,
            &linked_map,
            None,
            Some(LinkedProjection {
                projection: &projection,
                index: &index,
            }),
        )
        .expect("inline join");

        let level_one = &entity["outer"]["entity"];
        assert!(level_one.get("inner").is_some());
        assert!(level_one.get("name").is_none());
        let level_two = &level_one["inner"]["entity"];
        pretty_assertions::assert_eq!(level_two["id"], "urn:c");
        assert!(level_two.get("zone").is_some());
        assert!(level_two.get("name").is_none());
    }

    /// Clause 4.21: `omit=rel{name}` leaves `rel` on the linking entity and
    /// removes `name` from what it links to.
    #[test]
    fn inline_join_applies_a_nested_omit_to_the_linked_entity_only() {
        let target = parse_entity(json!({
            "id": "urn:t", "type": "T",
            "category": {"type": "Property", "value": "commercial"},
            "name": {"type": "Property", "value": "Depot"}
        }));
        let mut linked_map = IndexMap::new();
        linked_map.insert("urn:t".parse().expect("valid URI"), target);

        let projection = Projection {
            pick: Vec::new(),
            omit: vec![term("rel", vec![term("name", Vec::new())])],
        };
        let index = index(&["rel", "category", "name"]);

        let mut entity = json!({
            "id": "urn:a", "type": "A",
            "rel": {"type": "Relationship", "object": "urn:t"}
        });

        apply_inline_join(
            &mut entity,
            &linked_map,
            None,
            Some(LinkedProjection {
                projection: &projection,
                index: &index,
            }),
        )
        .expect("inline join");

        assert!(entity.get("rel").is_some());
        assert!(entity["rel"]["entity"].get("category").is_some());
        assert!(entity["rel"]["entity"].get("name").is_none());
    }

    #[test]
    fn inline_join_expands_relationship() {
        let linked_entity = parse_entity(json!({
            "id": "urn:ngsi-ld:Room:1",
            "type": "Room",
            "name": {"type": "Property", "value": "Conference Room"}
        }));
        let mut linked_map = IndexMap::new();
        linked_map.insert("urn:ngsi-ld:Room:1".parse().expect("valid URI"), linked_entity);

        let mut entity = json!({
            "id": "urn:ngsi-ld:Building:1",
            "type": "Building",
            "hasRoom": {"type": "Relationship", "object": "urn:ngsi-ld:Room:1"}
        });

        apply_inline_join(&mut entity, &linked_map, None, None).expect("inline join");
        assert!(entity["hasRoom"]["entity"].is_object());
        pretty_assertions::assert_eq!(entity["hasRoom"]["entity"]["id"], "urn:ngsi-ld:Room:1");
        pretty_assertions::assert_eq!(entity["hasRoom"]["entity"]["name"]["value"], "Conference Room");
    }

    #[test]
    fn inline_join_expands_list_relationship() {
        let room_a = parse_entity(json!({"id": "urn:a", "type": "Room", "name": {"type": "Property", "value": "A"}}));
        let room_b = parse_entity(json!({"id": "urn:b", "type": "Room", "name": {"type": "Property", "value": "B"}}));
        let mut linked_map = IndexMap::new();
        linked_map.insert("urn:a".parse().expect("valid URI"), room_a);
        linked_map.insert("urn:b".parse().expect("valid URI"), room_b);

        let mut entity = json!({
            "id": "urn:building", "type": "Building",
            "hasRooms": {"type": "ListRelationship", "objectList": ["urn:a", "urn:b"]}
        });

        apply_inline_join(&mut entity, &linked_map, None, None).expect("inline join");
        let entity_list = entity["hasRooms"]["entityList"].as_array().expect("entityList array");
        pretty_assertions::assert_eq!(entity_list.len(), 2);
    }

    #[test]
    fn inline_join_skips_unresolved() {
        let linked_map: IndexMap<Uri, Entity> = IndexMap::new();
        let mut entity = json!({
            "id": "urn:building", "type": "Building",
            "hasRoom": {"type": "Relationship", "object": "urn:ngsi-ld:Missing:1"}
        });

        apply_inline_join(&mut entity, &linked_map, None, None).expect("inline join");
        assert!(entity["hasRoom"].get("entity").is_none());
    }

    #[test]
    fn inline_join_handles_multi_attribute_array() {
        let room = parse_entity(json!({"id": "urn:a", "type": "Room", "name": {"type": "Property", "value": "A"}}));
        let mut linked_map = IndexMap::new();
        linked_map.insert("urn:a".parse().expect("valid URI"), room);

        let mut entity = json!({
            "id": "urn:building", "type": "Building",
            "hasRoom": [
                {"type": "Relationship", "object": "urn:a", "datasetId": "urn:ds:1"},
                {"type": "Relationship", "object": "urn:missing", "datasetId": "urn:ds:2"}
            ]
        });

        apply_inline_join(&mut entity, &linked_map, None, None).expect("inline join");
        let arr = entity["hasRoom"].as_array().expect("array");
        assert!(arr[0].get("entity").is_some());
        assert!(arr[1].get("entity").is_none());
    }

    #[test]
    fn inline_join_recursive() {
        let entity_c = parse_entity(json!({
            "id": "urn:c", "type": "T", "name": {"type": "Property", "value": "C"}
        }));
        let entity_b = parse_entity(json!({
            "id": "urn:b", "type": "T",
            "ref": {"type": "Relationship", "object": "urn:c"}
        }));
        let mut linked_map = IndexMap::new();
        linked_map.insert("urn:b".parse().expect("valid URI"), entity_b);
        linked_map.insert("urn:c".parse().expect("valid URI"), entity_c);

        let mut entity = json!({
            "id": "urn:a", "type": "T",
            "ref": {"type": "Relationship", "object": "urn:b"}
        });

        apply_inline_join(&mut entity, &linked_map, None, None).expect("inline join");
        pretty_assertions::assert_eq!(entity["ref"]["entity"]["id"], "urn:b");
        pretty_assertions::assert_eq!(entity["ref"]["entity"]["ref"]["entity"]["id"], "urn:c");
    }

    #[test]
    fn inline_join_breaks_a_self_reference() {
        // An entity that links to itself must render once and terminate, not
        // recurse until the stack overflows.
        let self_ref = parse_entity(json!({
            "id": "urn:a", "type": "T",
            "ref": {"type": "Relationship", "object": "urn:a"}
        }));
        let mut linked_map = IndexMap::new();
        linked_map.insert("urn:a".parse().expect("valid URI"), self_ref);

        let mut entity = json!({
            "id": "urn:a", "type": "T",
            "ref": {"type": "Relationship", "object": "urn:a"}
        });

        apply_inline_join(&mut entity, &linked_map, None, None).expect("inline join terminates");
        pretty_assertions::assert_eq!(entity["ref"]["entity"]["id"], "urn:a");
        // The self-reference inside the nested entity is left unexpanded.
        assert!(entity["ref"]["entity"]["ref"].get("entity").is_none());
    }

    #[test]
    fn inline_join_breaks_a_two_entity_cycle() {
        // A -> B -> A. Rendering must terminate; the back-reference from B to A
        // is left as a plain Relationship rather than expanded a second time.
        let entity_a = parse_entity(json!({
            "id": "urn:a", "type": "T",
            "ref": {"type": "Relationship", "object": "urn:b"}
        }));
        let entity_b = parse_entity(json!({
            "id": "urn:b", "type": "T",
            "ref": {"type": "Relationship", "object": "urn:a"}
        }));
        let mut linked_map = IndexMap::new();
        linked_map.insert("urn:a".parse().expect("valid URI"), entity_a);
        linked_map.insert("urn:b".parse().expect("valid URI"), entity_b);

        let mut entity = json!({
            "id": "urn:a", "type": "T",
            "ref": {"type": "Relationship", "object": "urn:b"}
        });

        apply_inline_join(&mut entity, &linked_map, None, None).expect("inline join terminates");
        pretty_assertions::assert_eq!(entity["ref"]["entity"]["id"], "urn:b");
        pretty_assertions::assert_eq!(entity["ref"]["entity"]["ref"]["entity"]["id"], "urn:a");
        // The cycle is broken: A reached through B is not expanded again.
        assert!(entity["ref"]["entity"]["ref"]["entity"]["ref"].get("entity").is_none());
    }
}
