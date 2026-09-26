//! Whether a stored entity is the one a clause 4.17 type selector names.
//!
//! Clauses 5.6.17.4 (Merge Entity) and 5.7.1.4 (Retrieve Entity) both know
//! their target as the entity whose identifier "and, where specified, type"
//! is equivalent, so an entity whose types fall outside the selector is not the
//! target at all.

use andromeda_ngsi_ld_model::expanded::node::expanded_object_ref;
use andromeda_ngsi_ld_query::type_selection::grammar::TypeExpr;
use serde_json::Value;

/// Whether an expanded entity's types satisfy an expanded clause 4.17 selector.
pub(crate) fn entity_satisfies(expanded: &Value, selector: &TypeExpr) -> bool {
    let types = expanded_object_ref(expanded)
        .and_then(|object| object.get("@type"))
        .and_then(Value::as_array)
        .map(|types| types.iter().filter_map(Value::as_str).collect::<Vec<_>>())
        .unwrap_or_default();
    selector.matches(&types)
}

#[cfg(test)]
mod tests {
    use crate::entity_type_match::entity_satisfies;
    use andromeda_ngsi_ld_query::type_selection::grammar::TypeExpr;
    use iri_rs::IriBuf;
    use serde_json::json;

    fn named(name: &str) -> TypeExpr {
        TypeExpr::Type(IriBuf::new(format!("https://example.org/{name}")).expect("valid IRI"))
    }

    #[test]
    fn an_entity_satisfies_a_selector_naming_one_of_its_types() {
        let entity = json!({"@id": "urn:ngsi-ld:Vehicle:1", "@type": ["https://example.org/Vehicle", "https://example.org/Asset"]});
        assert!(entity_satisfies(&entity, &named("Vehicle")));
        assert!(entity_satisfies(&entity, &TypeExpr::Or(vec![named("Building"), named("Asset")])));
        assert!(entity_satisfies(&entity, &TypeExpr::And(vec![named("Vehicle"), named("Asset")])));
    }

    #[test]
    fn an_entity_outside_the_selector_does_not_satisfy_it() {
        let entity = json!({"@id": "urn:ngsi-ld:Vehicle:1", "@type": ["https://example.org/Vehicle"]});
        assert!(!entity_satisfies(&entity, &named("Building")));
        assert!(!entity_satisfies(&entity, &TypeExpr::And(vec![named("Vehicle"), named("Asset")])));
    }

    #[test]
    fn an_entity_without_types_satisfies_no_named_type() {
        let entity = json!({"@id": "urn:ngsi-ld:Vehicle:1"});
        assert!(!entity_satisfies(&entity, &named("Vehicle")));
    }
}
