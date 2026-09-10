//! Expanding the bare Attribute Fragment of clauses 5.6.4 and 5.6.19.
//!
//! Partial Attribute Update and Replace Attribute are the two operations whose
//! payload is one attribute instance with no entity around it: the attribute's
//! name is in the request URI, not in the document. Expanding such a document
//! on its own loses that name, and JSON-LD makes the name load-bearing — a
//! term's definition governs what the members below it mean, through
//! `@container`, through type coercion, and through a property-scoped
//! `@context`.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.4 uses exactly that last mechanism. The core
//! `@context` declares `entityIdSealed` and `entityTypeSealed` only inside the
//! scoped context of the `ngsildproof` term, which is what makes the two seals
//! of clause 4.5.2.2 meaningful under that Property and undefined terms
//! everywhere else. A Fragment expanded loose never enters that scope, so both
//! seals resolve against the default `@vocab` instead and the write stores two
//! IRIs the specification does not define, under which no reader will find
//! them again.
//!
//! Putting the Fragment back under its own name before expansion is what
//! restores the scope. Nothing here is specific to `ngsildproof`: the same
//! restoration is what lets any term a context scopes mean on these two
//! operations what it means on every other write path.

use crate::{context_spec::ContextSpec, engine::processor::JsonLdEngine, error::JsonLdError};
use serde_json::{Map, Value};

/// Expand `fragment` as the value of the attribute named `attribute_name`.
///
/// Answers the attribute's expanded instances, with the name's own term
/// definition honoured. A Fragment carrying no member at all answers one
/// instance carrying none, which is what an attribute standing in the property
/// position of a document expands to; only a document with no attribute in it
/// answers an empty array.
///
/// # Errors
///
/// Returns [`JsonLdError::Expansion`] on an expansion failure, or a
/// context-loading variant when a context did not resolve.
pub async fn expand_attribute_fragment(engine: &JsonLdEngine, attribute_name: &str, fragment: Value, context: &ContextSpec) -> Result<Value, JsonLdError> {
    let mut document = Map::with_capacity(1);
    document.insert(attribute_name.to_owned(), fragment);
    let expanded = engine.expand(Value::Object(document), context).await?;
    Ok(instances_of(expanded))
}

/// The instances the expanded wrapper carries under its one attribute.
///
/// Expansion answers an array of node objects, and the wrapper contributes
/// exactly one node with exactly one member that is not a JSON-LD keyword — the
/// attribute. Reading it back by shape rather than by IRI is what keeps this
/// agnostic about how the name expanded, which a user `@context` decides.
fn instances_of(expanded: Value) -> Value {
    let Value::Array(nodes) = expanded else {
        return Value::Array(Vec::new());
    };
    let Some(Value::Object(node)) = nodes.into_iter().next() else {
        return Value::Array(Vec::new());
    };
    node.into_iter()
        .find(|(member, _)| !member.starts_with('@'))
        .map_or_else(|| Value::Array(Vec::new()), |(_, instances)| instances)
}

#[cfg(test)]
mod tests {
    use crate::{attribute_fragment::expand_attribute_fragment, context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use serde_json::json;

    /// An engine that resolves only what is embedded, never the network.
    fn offline_engine() -> JsonLdEngine {
        JsonLdEngine::builder().allow_remote(false).build().expect("engine builds")
    }

    #[tokio::test]
    async fn a_fragment_expands_to_the_instances_of_its_own_attribute() {
        let expanded = expand_attribute_fragment(&offline_engine(), "speed", json!({ "type": "Property", "value": 42 }), &ContextSpec::none())
            .await
            .expect("the fragment expands");

        pretty_assertions::assert_eq!(
            expanded,
            json!([{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{ "@value": 42 }]
            }])
        );
    }

    /// Clause 4.4 declares `entityIdSealed` and `entityTypeSealed` inside the
    /// scoped `@context` of the `ngsildproof` term alone, so only a Fragment
    /// expanded under that name resolves them to the IRIs clause 4.5.2.2 gives
    /// them. `entityTypeSealed` is declared `"@type": "@vocab"`, which is what
    /// turns its bare value into an IRI node rather than a string literal.
    #[tokio::test]
    async fn the_seals_of_clause_4_5_2_2_resolve_through_the_ngsildproof_scoped_context() {
        let expanded = expand_attribute_fragment(
            &offline_engine(),
            "ngsildproof",
            json!({
                "type": "Property",
                "entityIdSealed": "urn:ngsi-ld:Store:002",
                "entityTypeSealed": "Store",
                "value": { "proofValue": "zAAA" }
            }),
            &ContextSpec::none(),
        )
        .await
        .expect("the fragment expands");

        let instance = &expanded[0];
        pretty_assertions::assert_eq!(
            instance["https://uri.etsi.org/ngsi-ld/entityIdSealed"],
            json!([{ "@value": "urn:ngsi-ld:Store:002" }])
        );
        pretty_assertions::assert_eq!(
            instance["https://uri.etsi.org/ngsi-ld/entityTypeSealed"],
            json!([{ "@id": "https://uri.etsi.org/ngsi-ld/default-context/Store" }])
        );
    }

    /// The same two members under any other name are outside every scoped
    /// context the core `@context` declares, so they resolve against `@vocab` —
    /// which is the shape `andromeda_ngsi_ld_representation::proof_seal_admission`
    /// refuses before a payload ever reaches expansion.
    #[tokio::test]
    async fn a_seal_written_under_another_name_resolves_against_the_default_vocabulary() {
        let expanded = expand_attribute_fragment(
            &offline_engine(),
            "speed",
            json!({ "type": "Property", "value": 1, "entityIdSealed": "urn:ngsi-ld:Store:002" }),
            &ContextSpec::none(),
        )
        .await
        .expect("the fragment expands");

        assert!(expanded[0].get("https://uri.etsi.org/ngsi-ld/entityIdSealed").is_none());
        pretty_assertions::assert_eq!(
            expanded[0]["https://uri.etsi.org/ngsi-ld/default-context/entityIdSealed"],
            json!([{ "@value": "urn:ngsi-ld:Store:002" }])
        );
    }

    /// A Fragment with no member is still an attribute in the property
    /// position, so it expands to one instance carrying nothing rather than to
    /// no instance at all.
    #[tokio::test]
    async fn an_empty_fragment_answers_one_instance_carrying_no_member() {
        let expanded = expand_attribute_fragment(&offline_engine(), "speed", json!({}), &ContextSpec::none())
            .await
            .expect("the fragment expands");

        pretty_assertions::assert_eq!(expanded, json!([{}]));
    }

    /// A Fragment that is not an attribute node at all leaves the wrapper with
    /// nothing under its one member, and no instance is answered.
    #[tokio::test]
    async fn a_fragment_that_is_not_a_node_answers_an_empty_array() {
        let expanded = expand_attribute_fragment(&offline_engine(), "speed", json!(null), &ContextSpec::none())
            .await
            .expect("the fragment expands");

        pretty_assertions::assert_eq!(expanded, json!([]));
    }

    /// A multi-instance Fragment keeps every instance, in the order written.
    #[tokio::test]
    async fn every_instance_of_a_multi_instance_fragment_is_answered() {
        let expanded = expand_attribute_fragment(
            &offline_engine(),
            "speed",
            json!([
                { "type": "Property", "value": 1, "datasetId": "urn:ngsi-ld:Dataset:a" },
                { "type": "Property", "value": 2, "datasetId": "urn:ngsi-ld:Dataset:b" }
            ]),
            &ContextSpec::none(),
        )
        .await
        .expect("the fragment expands");

        pretty_assertions::assert_eq!(expanded.as_array().map(Vec::len), Some(2));
        pretty_assertions::assert_eq!(expanded[0]["https://uri.etsi.org/ngsi-ld/hasValue"], json!([{ "@value": 1 }]));
        pretty_assertions::assert_eq!(expanded[1]["https://uri.etsi.org/ngsi-ld/hasValue"], json!([{ "@value": 2 }]));
    }
}
