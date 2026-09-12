//! The attribute half of the clause 5.8.6 `notification.pick` /
//! `notification.omit` projection, applied to an expanded entity before it is
//! compacted for delivery.
//!
//! An expanded entity keys its attributes by IRI, which is how the compiled
//! [`Projection`] names them too, so the clause 4.21 attribute rule is a key
//! comparison here. `id`, `type` and `scope` are not attributes and are not
//! decided here: they exist as such only once compaction has turned `@id` and
//! `@type` into them, so `StructuralProjection` prunes them afterwards, by the
//! same rule a query response uses. Clause 5.8.6 reduces "every Entity within
//! the payload body ... down to only contain the defined entity members
//! listed", which is clause 5.7.1.5's sentence verbatim, so the two must not
//! read it differently.

use andromeda_ngsi_ld_query::projection::{compiled::Projection, term::ProjectionTerm};
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use serde_json::Value;

/// Apply a compiled `pick`/`omit` projection to one expanded entity node.
///
/// With a non-empty `pick` only the picked attributes survive; a non-empty
/// `omit` removes the attributes it names bare. The JSON-LD keywords, the
/// clause 4.8 system timestamps and the `scope` member are never attributes
/// and are left alone here.
pub(crate) fn apply_notification_projection(entity: &mut Value, projection: &Projection) {
    let Some(object) = entity.as_object_mut() else {
        return;
    };
    if !projection.pick.is_empty() {
        object.retain(|key, _| is_never_projected(key) || kept_by_pick(key, &projection.pick));
    }
    if !projection.omit.is_empty() {
        object.retain(|key, _| is_never_projected(key) || !removed_by_omit(key, &projection.omit));
    }
}

/// Whether a key of an expanded entity is outside this half of the clause 4.21
/// rule: the JSON-LD keywords (`@id`, `@type`) and `scope`, which the
/// structural pass decides after compaction, and the system timestamps, which
/// `sysAttrs` governs rather than a projection.
fn is_never_projected(key: &str) -> bool {
    key.starts_with('@')
        || is_scope(key)
        || key == expanded_properties::CREATED_AT.as_str()
        || key == expanded_properties::MODIFIED_AT.as_str()
        || key == expanded_properties::DELETED_AT.as_str()
        || key == expanded_properties::EXPIRES_AT.as_str()
}

/// Whether `key` is the expanded `scope` member.
fn is_scope(key: &str) -> bool {
    key == expanded_properties::SCOPE.as_str()
}

/// Clause 4.21: a non-empty `pick` keeps the attributes it names.
fn kept_by_pick(key: &str, pick: &[ProjectionTerm]) -> bool {
    pick.iter()
        .any(|term| matches!(term, ProjectionTerm::Attribute { iri, .. } if iri.as_str() == key))
}

/// Clause 4.21: `omit` removes the attributes it names bare. `omit=rel{a}`
/// omits `a` inside the entity `rel` links to, not `rel` itself, so a term
/// carrying a linked sub-projection removes nothing here.
fn removed_by_omit(key: &str, omit: &[ProjectionTerm]) -> bool {
    omit.iter()
        .any(|term| matches!(term, ProjectionTerm::Attribute { iri, linked } if iri.as_str() == key && linked.is_empty()))
}

#[cfg(test)]
mod tests {
    use crate::processor::payload::projection::apply_notification_projection;
    use andromeda_ngsi_ld_query::projection::{compiled::Projection, term::ProjectionTerm};
    use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
    use iri_rs::IriBuf;
    use serde_json::{Value, json};

    const SPEED: &str = "https://example.com/speed";
    const COLOUR: &str = "https://example.com/colour";

    fn attribute(iri: &str, linked: Vec<ProjectionTerm>) -> ProjectionTerm {
        ProjectionTerm::Attribute {
            iri: IriBuf::new(iri.to_owned()).expect("valid IRI"),
            linked,
        }
    }

    fn expanded_entity() -> Value {
        json!({
            "@id": "urn:ngsi-ld:Vehicle:1",
            "@type": ["https://example.com/Vehicle"],
            SPEED: [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 20}]}],
            COLOUR: [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": "red"}]}],
            expanded_properties::SCOPE.as_str(): [{"@value": "/Madrid"}],
            expanded_properties::CREATED_AT.as_str(): [{"@value": "2024-01-01T00:00:00Z"}]
        })
    }

    fn keys(entity: &Value) -> Vec<&str> {
        entity.as_object().expect("object").keys().map(String::as_str).collect()
    }

    #[test]
    fn an_empty_projection_leaves_the_entity_untouched() {
        let mut entity = expanded_entity();
        apply_notification_projection(&mut entity, &Projection::default());
        pretty_assertions::assert_eq!(entity, expanded_entity());
    }

    /// Clause 5.8.6 / 4.21: a pick reduces the entity to the picked attribute.
    /// The keywords, `scope` and the system timestamps are not this pass's to
    /// decide and stay for the one that owns them.
    #[test]
    fn a_pick_keeps_only_the_picked_attribute_beside_what_this_pass_does_not_decide() {
        let mut entity = expanded_entity();
        let projection = Projection::builder().pick(vec![attribute(SPEED, Vec::new())]).build();

        apply_notification_projection(&mut entity, &projection);

        pretty_assertions::assert_eq!(
            keys(&entity),
            vec![
                "@id",
                "@type",
                SPEED,
                expanded_properties::CREATED_AT.as_str(),
                expanded_properties::SCOPE.as_str()
            ]
        );
    }

    /// A pick that names only a structural keyword removes every attribute and
    /// leaves the three structural members for the pass that owns them.
    #[test]
    fn a_pick_naming_only_a_keyword_removes_every_attribute() {
        let mut entity = expanded_entity();
        let projection = Projection::builder().pick(vec![ProjectionTerm::Scope]).build();

        apply_notification_projection(&mut entity, &projection);

        assert!(entity.get(SPEED).is_none() && entity.get(COLOUR).is_none(), "{entity}");
        assert!(
            entity.get(expanded_properties::SCOPE.as_str()).is_some(),
            "`scope` is settled after compaction, not here: {entity}"
        );
    }

    #[test]
    fn an_omit_removes_the_named_attribute_and_leaves_the_structural_members() {
        let mut entity = expanded_entity();
        let projection = Projection::builder().omit(vec![attribute(COLOUR, Vec::new())]).build();
        apply_notification_projection(&mut entity, &projection);
        assert!(entity.get(COLOUR).is_none() && entity.get(SPEED).is_some(), "{entity}");
        assert!(entity.get(expanded_properties::SCOPE.as_str()).is_some(), "{entity}");

        let mut entity = expanded_entity();
        let projection = Projection::builder().omit(vec![ProjectionTerm::Scope]).build();
        apply_notification_projection(&mut entity, &projection);
        assert!(
            entity.get(expanded_properties::SCOPE.as_str()).is_some(),
            "`scope` is settled after compaction, not here: {entity}"
        );
        assert!(entity.get(SPEED).is_some() && entity.get(COLOUR).is_some(), "{entity}");
    }

    /// Clause 4.21: `omit=rel{a}` reaches into the linked entity and leaves
    /// the relationship itself in place.
    #[test]
    fn an_omit_term_with_a_linked_sub_projection_keeps_the_attribute_itself() {
        let mut entity = expanded_entity();
        let projection = Projection::builder().omit(vec![attribute(SPEED, vec![attribute(COLOUR, Vec::new())])]).build();

        apply_notification_projection(&mut entity, &projection);

        assert!(entity.get(SPEED).is_some(), "{entity}");
    }

    #[test]
    fn a_non_object_node_is_left_alone() {
        let mut entity = json!("not an entity");
        apply_notification_projection(&mut entity, &Projection::builder().pick(vec![ProjectionTerm::Id]).build());
        pretty_assertions::assert_eq!(entity, json!("not an entity"));
    }
}
