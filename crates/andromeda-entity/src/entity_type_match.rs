//! Whether a stored entity is the one a clause 4.17 type selector names.
//!
//! Clauses 5.6.17.4 (Merge Entity) and 5.7.1.4 (Retrieve Entity) both know
//! their target as the entity whose identifier "and, where specified, type"
//! is equivalent, so an entity whose types fall outside the selector is not the
//! target at all.

use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
use andromeda_ngsi_ld_query::type_selection::grammar::TypeExpr;

/// Whether a stored entity's types satisfy an expanded clause 4.17 selector.
pub(crate) fn entity_satisfies(state: &EntityState, selector: &TypeExpr) -> bool {
    types_satisfy(state.types().as_slice(), selector)
}

/// Whether an entity's `types` satisfy an expanded clause 4.17 selector.
pub(crate) fn types_satisfy(types: &[EntityTypeName], selector: &TypeExpr) -> bool {
    let names: Vec<&str> = types.iter().map(EntityTypeName::as_str).collect();
    selector.matches(&names)
}

#[cfg(test)]
mod tests {
    use crate::entity_type_match::entity_satisfies;
    use andromeda_ngsi_ld_entity::entity_state::EntityState;
    use andromeda_ngsi_ld_query::type_selection::grammar::TypeExpr;
    use iri_rs::IriBuf;
    use serde_json::json;

    fn named(name: &str) -> TypeExpr {
        TypeExpr::Type(IriBuf::new(format!("https://example.org/{name}")).expect("valid IRI"))
    }

    fn typed(types: &[&str]) -> EntityState {
        let document = json!([{"@id": "urn:ngsi-ld:Vehicle:1", "@type": types}]);
        EntityState::from_slice(&serde_json::to_vec(&document).expect("JSON")).expect("a stored state")
    }

    #[test]
    fn an_entity_satisfies_a_selector_naming_one_of_its_types() {
        let entity = typed(&["https://example.org/Vehicle", "https://example.org/Asset"]);
        assert!(entity_satisfies(&entity, &named("Vehicle")));
        assert!(entity_satisfies(&entity, &TypeExpr::Or(vec![named("Building"), named("Asset")])));
        assert!(entity_satisfies(&entity, &TypeExpr::And(vec![named("Vehicle"), named("Asset")])));
    }

    #[test]
    fn an_entity_outside_the_selector_does_not_satisfy_it() {
        let entity = typed(&["https://example.org/Vehicle"]);
        assert!(!entity_satisfies(&entity, &named("Building")));
        assert!(!entity_satisfies(&entity, &TypeExpr::And(vec![named("Vehicle"), named("Asset")])));
    }

    /// Clause 4.17: the wildcard selects an entity of any type.
    #[test]
    fn every_entity_satisfies_the_wildcard() {
        assert!(entity_satisfies(&typed(&["https://example.org/Vehicle"]), &TypeExpr::Wildcard));
    }
}
