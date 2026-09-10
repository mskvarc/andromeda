//! Compacting one expanded NGSI-LD name back to the term a context spells it
//! with.
//!
//! An Entity Type name and an Attribute name share the clause 4.6.2 grammar and
//! both sit in vocabulary position, so [`compact_type_iri`] and
//! [`compact_attr_iri`] are two names for one operation. They stay separate
//! entry points because the domains are: a caller knows which of the two it
//! holds, and a future divergence between them would have somewhere to land.
//!
//! Compaction is defined over documents, not over names, so the first step
//! builds the smallest document that puts the IRI where compaction acts on it
//! and reads the term back out. That step answers whatever JSON-LD's own term
//! selection answers, which is the whole rule for every name whose term
//! definition coerces nothing. A term that coerces a datatype is the exception,
//! and clause C.6 is what says so: such a term is not a candidate for a
//! reified value, but it still names the Attribute, and Table 5.2.28-1
//! conditions the short name on membership of the `@context` and on nothing
//! else. The second step therefore asks the request's own contexts which term
//! they declare for the IRI.
//!
//! An IRI neither step shortens is answered verbatim: clause 4.5.1 permits a
//! rendering to carry a full IRI when no term abbreviates it, so failing here
//! would refuse a response that is valid.

use crate::{
    context_spec::ContextSpec,
    core_context::is_ngsi_ld_core_context_url,
    engine::{
        context_url::is_full_iri,
        options::{CompactOptions, ContextStripping},
        processor::JsonLdEngine,
    },
};
use iri_rs::{Iri, iri};
use serde_json::Value;
use std::collections::{BTreeSet, HashSet};

/// The options every compaction here uses: the caller wants a term, not a
/// document, so the `@context` is never kept.
const TERM_ONLY: CompactOptions = CompactOptions {
    strip_context: ContextStripping::Strip,
};

/// The identifier the probe document carries, which is never the answer.
static PROBE_ID: Iri<&'static str> = iri!("urn:ngsi-ld:_:dummy");

/// The core term that carries a name in vocabulary position.
///
/// The core context declares `attributeList` with `"@type": "@vocab"`, which is
/// exactly the position an NGSI-LD name occupies: its value is a name to be
/// compacted against the vocabulary rather than a value to be coerced. The term
/// is protected (clause C.9), so no request context can redefine it and change
/// what the probe measures.
static NAME_CARRIER: Iri<&'static str> = iri!("https://uri.etsi.org/ngsi-ld/attributeList");

/// How many context documents one name lookup will read before giving up.
///
/// A remote context may reference further contexts; the visited set already
/// closes cycles, and this bounds the breadth of a legitimately deep chain.
const MAX_CONTEXT_DOCUMENTS: usize = 64;

/// Compact an entity type IRI to the term a context spells it with.
///
/// Answers the IRI verbatim when it does not compact, or when it is not one
/// of the schemes an NGSI-LD type can use.
pub async fn compact_type_iri(engine: &JsonLdEngine, iri: &str, context: &ContextSpec) -> String {
    compact_name(engine, iri, context).await
}

/// Compact an attribute IRI to the term a context spells it with.
///
/// Answers the IRI verbatim when it does not compact, or when it is not one
/// of the schemes an NGSI-LD attribute name can use.
pub async fn compact_attr_iri(engine: &JsonLdEngine, iri: &str, context: &ContextSpec) -> String {
    compact_name(engine, iri, context).await
}

/// The short name `context` gives an expanded NGSI-LD name, or the name itself.
async fn compact_name(engine: &JsonLdEngine, iri: &str, context: &ContextSpec) -> String {
    if !is_full_iri(iri) {
        return iri.to_string();
    }
    if let Some(term) = vocabulary_position_term(engine, iri, context).await {
        return term;
    }
    match declared_term(engine, iri, context).await {
        Some(term) => term,
        None => iri.to_string(),
    }
}

/// The term JSON-LD's own compaction selects for the IRI in vocabulary
/// position, or `None` when it selects none.
async fn vocabulary_position_term(engine: &JsonLdEngine, iri: &str, context: &ContextSpec) -> Option<String> {
    let document = serde_json::json!([{ "@id": PROBE_ID.as_str(), NAME_CARRIER.as_str(): [{ "@id": iri }] }]);

    let compacted = match engine.compact(document, context, TERM_ONLY).await {
        Ok(compacted) => compacted,
        Err(error) => {
            tracing::debug!(error = %error, iri, "name did not compact; answering it verbatim");
            return None;
        }
    };
    // The compacted document holds the probe identifier and the carrier; the
    // carrier's value is the name, collapsed to a bare string by compaction.
    let carried = compacted
        .as_object()
        .and_then(|object| object.iter().find(|(key, _)| *key != "id" && !key.starts_with('@')))
        .map(|(_, value)| value)?;
    let name = match carried {
        Value::String(name) => name.as_str(),
        Value::Array(items) => items.first().and_then(Value::as_str)?,
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Object(_) => return None,
    };
    (name != iri).then(|| name.to_string())
}

/// The term the request's own contexts declare for the IRI.
///
/// Table 5.2.28-1 makes membership of the `@context` the whole condition for a
/// short name, and clause C.6 names the case that reaches here: a term
/// definition that coerces a datatype cannot be selected for a value JSON-LD
/// would then have to re-type, yet the term is what the context calls the
/// Attribute. Only the contexts the request supplied are searched — a name the
/// core context declares is never hidden by a coercion, so it has already been
/// answered above.
///
/// Two terms declaring the same IRI are ordered as JSON-LD orders compaction
/// candidates: the shortest first, then the lexicographically least.
async fn declared_term(engine: &JsonLdEngine, iri: &str, context: &ContextSpec) -> Option<String> {
    let prepared_context = engine.prepare_term_context(context);
    let mut best: Option<Box<str>> = None;
    for name in declared_names(engine, context).await {
        let expands_to_iri = engine
            .expand_term(&name, &prepared_context)
            .await
            .is_ok_and(|expanded| expanded.as_str() == iri);
        if expands_to_iri && best.as_ref().is_none_or(|current| preferred(&name, current)) {
            best = Some(name);
        }
    }
    best.map(String::from)
}

/// Whether `candidate` is the term JSON-LD would prefer over `current`.
fn preferred(candidate: &str, current: &str) -> bool {
    (candidate.len(), candidate) < (current.len(), current)
}

/// The term names the contexts a request supplied declare.
///
/// Over-collecting is harmless — a name that is not a term expands through the
/// active `@vocab` and matches only the IRI it would really name — so the walk
/// reads a context document for its declarations without repeating the
/// processor's own context processing. The core context is not walked: its
/// declarations are the ones vocabulary-position compaction already answers.
async fn declared_names(engine: &JsonLdEngine, context: &ContextSpec) -> BTreeSet<Box<str>> {
    let mut pending: Vec<Value> = context.link_contexts().iter().map(|url| Value::String(url.to_string())).collect();
    if let Some(body) = context.body_context() {
        pending.push(body.clone()); // clone: the walk owns the values it descends into
    }

    let mut visited: HashSet<String> = HashSet::new();
    let mut names: BTreeSet<Box<str>> = BTreeSet::new();
    while let Some(value) = pending.pop() {
        match value {
            Value::String(url) => {
                if visited.len() >= MAX_CONTEXT_DOCUMENTS {
                    break;
                }
                if !visited.insert(url.clone()) {
                    // clone: the visited set owns its entry, the fetch below borrows the original
                    continue;
                }
                if Iri::<&str>::parse(url.as_str()).is_ok_and(is_ngsi_ld_core_context_url) {
                    continue;
                }
                match engine.resolve_context(&url).await {
                    Ok(document) => {
                        if let Some(declarations) = document.get("@context") {
                            pending.push(declarations.clone()); // clone: the walk owns the values it descends into
                        }
                    }
                    Err(error) => tracing::debug!(error = %error, url, "a context did not resolve while looking for a declared name"),
                }
            }
            Value::Array(items) => pending.extend(items),
            Value::Object(declarations) => match declarations.get("@context") {
                // A hosted context document wraps its declarations.
                Some(nested) => pending.push(nested.clone()), // clone: the walk owns the values it descends into
                None => names.extend(
                    declarations
                        .iter()
                        .filter(|(term, declaration)| !term.starts_with('@') && !declaration.is_null())
                        .map(|(term, _)| Box::from(term.as_str())),
                ),
            },
            Value::Null | Value::Bool(_) | Value::Number(_) => {}
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use crate::{
        compaction::{compact_attr_iri, compact_type_iri, preferred},
        context_spec::ContextSpec,
        engine::processor::JsonLdEngine,
    };
    use andromeda_ngsi_ld_vocabulary::expanded::classes;
    use serde_json::json;

    /// An engine that resolves only what is embedded, never the network.
    fn offline_engine() -> JsonLdEngine {
        JsonLdEngine::builder().allow_remote(false).build().expect("engine builds")
    }

    /// The clause C.2.4 vocabulary, carried in the request body so the test
    /// needs no network.
    fn example_context() -> ContextSpec {
        ContextSpec::none().with_body_context(Some(json!({
            "Vehicle": "http://example.org/vehicle/Vehicle",
            "brandName": "http://example.org/vehicle/brandName",
            "isParked": {"@type": "@id", "@id": "http://example.org/common/isParked"},
            "testedAt": {"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@id": "http://example.org/test/testedAt"}
        })))
    }

    #[tokio::test]
    async fn a_name_that_is_not_an_iri_is_answered_verbatim() {
        let engine = offline_engine();
        let context = ContextSpec::none();

        pretty_assertions::assert_eq!(compact_type_iri(&engine, "Vehicle", &context).await, "Vehicle");
        pretty_assertions::assert_eq!(compact_attr_iri(&engine, "speed", &context).await, "speed");
    }

    #[tokio::test]
    async fn a_core_context_type_iri_compacts_back_to_its_term() {
        let compacted = compact_type_iri(&offline_engine(), classes::PROPERTY.as_str(), &ContextSpec::none()).await;
        pretty_assertions::assert_eq!(compacted, "Property");
    }

    #[tokio::test]
    async fn an_iri_no_context_abbreviates_is_answered_verbatim() {
        let engine = offline_engine();
        let context = ContextSpec::core_only();
        let iri = "https://example.com/vocab#unknownTerm";

        pretty_assertions::assert_eq!(compact_type_iri(&engine, iri, &context).await, iri);
        pretty_assertions::assert_eq!(compact_attr_iri(&engine, iri, &context).await, iri);
    }

    /// Table 5.2.28-1 conditions the short name on membership of the
    /// `@context`; a `@type: @id` term definition is membership, so the name
    /// compacts exactly as a plain declaration does.
    #[tokio::test]
    async fn a_term_declared_with_an_identifier_coercion_still_names_its_attribute() {
        let engine = offline_engine();
        let context = example_context();

        pretty_assertions::assert_eq!(compact_attr_iri(&engine, "http://example.org/common/isParked", &context).await, "isParked");
        pretty_assertions::assert_eq!(compact_attr_iri(&engine, "http://example.org/vehicle/brandName", &context).await, "brandName");
    }

    /// Clause C.6: a term coercing a datatype is not a candidate for a reified
    /// value, but it is still what the `@context` calls the Attribute.
    #[tokio::test]
    async fn a_term_declared_with_a_datatype_coercion_still_names_its_attribute() {
        let engine = offline_engine();

        pretty_assertions::assert_eq!(
            compact_attr_iri(&engine, "http://example.org/test/testedAt", &example_context()).await,
            "testedAt"
        );
    }

    /// A name no supplied context declares stays a Fully Qualified Name, which
    /// is what clauses C.5.10 and C.5.11 print for the one attribute their
    /// `@context` leaves out.
    #[tokio::test]
    async fn a_name_the_supplied_context_does_not_declare_stays_fully_qualified() {
        let engine = offline_engine();
        let iri = "http://example.org/parking/status";

        pretty_assertions::assert_eq!(compact_attr_iri(&engine, iri, &example_context()).await, iri);
    }

    /// An entity type name takes the same route as an attribute name, so the
    /// two surfaces cannot answer the same `@context` differently.
    #[tokio::test]
    async fn an_entity_type_name_compacts_through_the_same_declaration() {
        let engine = offline_engine();

        pretty_assertions::assert_eq!(
            compact_type_iri(&engine, "http://example.org/vehicle/Vehicle", &example_context()).await,
            "Vehicle"
        );
    }

    /// Two terms declaring one IRI are ordered as JSON-LD orders compaction
    /// candidates: shortest first, then lexicographically least.
    #[tokio::test]
    async fn the_shortest_declaration_of_one_iri_is_the_name_rendered() {
        let engine = offline_engine();
        let context = ContextSpec::none().with_body_context(Some(json!({
            "zz": {"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@id": "http://example.org/test/testedAt"},
            "aaa": {"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@id": "http://example.org/test/testedAt"}
        })));

        pretty_assertions::assert_eq!(compact_attr_iri(&engine, "http://example.org/test/testedAt", &context).await, "zz");
    }

    #[test]
    fn a_shorter_candidate_wins_and_a_tie_is_broken_lexicographically() {
        assert!(preferred("ab", "abc"));
        assert!(!preferred("abc", "ab"));
        assert!(preferred("ab", "ba"));
        assert!(!preferred("ba", "ab"));
    }
}
