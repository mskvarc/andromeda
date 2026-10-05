//! Narrowing a compact entity's attributes down to what the client asked for
//! (clause 4.21 `pick` and `omit`, clause 5.7.2 `attrs`).
//!
//! An entity read from the local store is narrowed before it is compacted
//! (`crate::query::state_projection`); this narrows an entity that arrives
//! compact -- a Linked Entity another broker returned, or an entity on its way
//! to a subscriber -- whose member names have to be expanded to be matched.

use crate::{error::EntityError, query::attribute_expansion::AttributeExpansions};
use andromeda_linked_retrieval::{keyword::Keyword, narrowing::list::ListInForce};
use andromeda_ngsi_ld_model::entity::Entity;

/// Narrow one compact entity to the clause 4.21 list in force.
///
/// The attribute members the list does not return are removed, and the list's
/// keyword rule is applied to `scope`. `id` and `type` are members of the
/// *rendered* representation, not of the typed [`Entity`], so their half of
/// the rule belongs to the renderer.
///
/// Public because a subscription's `notification.pick`/`omit` and
/// `notification.attributes` (clause 5.8.6) are the same list applied to an
/// entity on its way to a subscriber rather than to a client.
///
/// # Errors
///
/// Returns [`EntityError`] when an attribute member name cannot be expanded
/// through the context.
pub async fn narrow_compact_entity(entity: &mut Entity, list: ListInForce<'_>, expansions: &mut AttributeExpansions<'_>) -> Result<(), EntityError> {
    if list.is_unrestricted() {
        return Ok(());
    }
    expansions.cover(entity).await?;
    entity.retain_attribute_members(|name| list.returns_attribute(name, expansions.index()));
    if !list.keeps_keyword(Keyword::Scope) {
        entity.scope = None;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::query::{attribute_expansion::AttributeExpansions, attribute_projection::narrow_compact_entity};
    use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use andromeda_linked_retrieval::narrowing::list::ListInForce;
    use andromeda_ngsi_ld_model::entity::Entity;
    use andromeda_ngsi_ld_query::projection::{compiled::Projection, term::ProjectionTerm};
    use iri_rs::IriBuf;
    use serde_json::json;

    const SPEED: &str = "https://example.com/speed";
    const OWNER: &str = "https://example.com/owner";
    const NAME: &str = "https://example.com/name";

    fn engine() -> JsonLdEngine {
        JsonLdEngine::builder().allow_remote(false).build().expect("engine builds")
    }

    fn context() -> ContextSpec {
        ContextSpec::from_body(json!({ "speed": SPEED, "owner": OWNER, "name": NAME }))
    }

    fn entity() -> Entity {
        serde_json::from_value(json!({
            "id": "urn:ngsi-ld:Building:1",
            "type": "Building",
            "scope": "/Madrid",
            "speed": {"type": "Property", "value": 1},
            "owner": {"type": "Relationship", "object": "urn:ngsi-ld:Person:1"}
        }))
        .expect("entity")
    }

    fn iri(value: &str) -> IriBuf {
        IriBuf::new(value.to_owned()).expect("valid IRI")
    }

    fn attribute(value: &str, linked: Vec<ProjectionTerm>) -> ProjectionTerm {
        ProjectionTerm::Attribute { iri: iri(value), linked }
    }

    async fn narrowed(list: ListInForce<'_>) -> Entity {
        let engine = engine();
        let prepared = engine.prepare_term_context(&context());
        let mut expansions = AttributeExpansions::new(&engine, &prepared);
        let mut entity = entity();
        narrow_compact_entity(&mut entity, list, &mut expansions).await.expect("narrows");
        entity
    }

    fn names(entity: &Entity) -> Vec<&str> {
        entity.attribute_member_names().collect()
    }

    /// Clause 4.21: a `pick` keeps the attributes it names and, not naming the
    /// `scope` keyword, drops `scope`.
    #[tokio::test]
    async fn a_pick_keeps_its_attributes_and_drops_an_unnamed_scope() {
        let projection = Projection {
            pick: vec![attribute(SPEED, Vec::new())],
            omit: Vec::new(),
        };
        let entity = narrowed(ListInForce::of(Some(&projection), &[])).await;
        pretty_assertions::assert_eq!(names(&entity), vec!["speed"]);
        assert!(entity.scope.is_none());
    }

    /// Clause 4.21: a bare `omit` removes the attribute, an `omit` carrying a
    /// linked sub-projection does not, and `scope` survives an `omit` not
    /// naming it.
    #[tokio::test]
    async fn an_omit_removes_only_its_bare_terms() {
        let projection = Projection {
            pick: Vec::new(),
            omit: vec![attribute(SPEED, Vec::new()), attribute(OWNER, vec![attribute(NAME, Vec::new())])],
        };
        let entity = narrowed(ListInForce::of(Some(&projection), &[])).await;
        pretty_assertions::assert_eq!(names(&entity), vec!["owner"]);
        assert!(entity.scope.is_some());
    }

    /// Clause 4.21: `scope` is a keyword `pick` may keep and `omit` remove.
    #[tokio::test]
    async fn the_scope_keyword_decides_the_scope() {
        let kept = Projection {
            pick: vec![ProjectionTerm::Scope],
            omit: Vec::new(),
        };
        assert!(narrowed(ListInForce::of(Some(&kept), &[])).await.scope.is_some());
        let omitted = Projection {
            pick: Vec::new(),
            omit: vec![ProjectionTerm::Scope],
        };
        assert!(narrowed(ListInForce::of(Some(&omitted), &[])).await.scope.is_none());
    }

    /// Table 5.2.14.1-1: `notification.attributes` is "a synonym for pick,
    /// except that id, type, scope are not allowed", so it keeps the named
    /// attributes and drops `scope`.
    #[tokio::test]
    async fn an_attribute_list_keeps_its_attributes_and_drops_the_scope() {
        let wanted = [iri(OWNER)];
        let entity = narrowed(ListInForce::of(None, &wanted)).await;
        pretty_assertions::assert_eq!(names(&entity), vec!["owner"]);
        assert!(entity.scope.is_none());
    }

    /// An unrestricted list narrows nothing.
    #[tokio::test]
    async fn an_unrestricted_list_leaves_the_entity_whole() {
        let entity = narrowed(ListInForce::Unrestricted).await;
        pretty_assertions::assert_eq!(names(&entity), vec!["owner", "speed"]);
        assert!(entity.scope.is_some());
    }
}
