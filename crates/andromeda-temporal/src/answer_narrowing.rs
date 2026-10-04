//! A Context Source's temporal answer, narrowed to the members a split query's
//! filters read and laid out as the Temporal Evolution a write would carry.
//!
//! Clause 5.7.4.4 judges an entity held in several parts on the union of its
//! parts, so the part a source answered with is read for exactly the leaves of
//! the query's value, geo and scope filters. A member none of them reads is not
//! read at all: whatever it holds cannot change the outcome, and a member the
//! write expansion would refuse cannot then keep the readable ones from being
//! judged. A kept member is carried as the source wrote it: a `temporalValues`
//! attribute (clause 4.5.9) is unfolded as a written one is, when the answer
//! is read as a Temporal Evolution.

use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::{entity_selection::EntitySelection, filter::referenced_attributes::referenced_attributes, term_expander::TermExpander};
use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties;
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};

/// The members of a temporal representation every narrowing keeps: what
/// names the entity.
const IDENTIFYING_MEMBERS: [&str; 2] = ["id", "type"];

/// Narrows the answers of one split query to the members its filters read.
///
/// The decision for a member key is memoised, so a key every answered entity
/// carries is expanded once per query rather than once per entity.
pub(crate) struct AnswerNarrowing<'q, X: TermExpander> {
    /// The expander the answers' compact keys are read under: the request's
    /// own `@context`, which the answers were compacted against.
    expander: &'q X,
    /// The expanded names of the attributes the filters read.
    read: HashSet<&'q str>,
    /// Whether a scope requirement reads the entity's Scope (clause 4.19).
    reads_scope: bool,
    /// Whether each compact member key met so far names an attribute read.
    decided: HashMap<String, bool>,
}

impl<'q, X: TermExpander> AnswerNarrowing<'q, X> {
    /// The narrowing to the members `selection`'s filters read, compact keys
    /// read under `expander`.
    pub(crate) fn new(selection: &'q EntitySelection, expander: &'q X) -> AnswerNarrowing<'q, X> {
        let mut read: HashSet<&'q str> = selection
            .q_expr
            .as_ref()
            .map(|expr| referenced_attributes(expr).into_iter().map(AttributeName::as_str).collect())
            .unwrap_or_default();
        read.extend(selection.geo.as_ref().map(|geo| geo.geoproperty.as_str()));
        AnswerNarrowing {
            expander,
            read,
            reads_scope: selection.scope_q.is_some(),
            decided: HashMap::new(),
        }
    }

    /// `answer` reduced to the members the filters read; an answer that is no
    /// entity object is left for the expansion to refuse.
    pub(crate) async fn narrow(&mut self, answer: &Value) -> Value {
        let Some(members) = answer.as_object() else {
            return answer.clone(); // clone: the answer stays as the merged response carries it
        };
        let mut kept = Map::with_capacity(members.len());
        for (key, value) in members {
            let keep = if IDENTIFYING_MEMBERS.contains(&key.as_str()) {
                true
            } else if key == compact_properties::SCOPE {
                self.reads_scope
            } else {
                self.reads(key).await
            };
            if keep {
                // clone: the answer stays as the merged response carries it
                kept.insert(key.clone(), value.clone());
            }
        }
        Value::Object(kept)
    }

    /// Whether the compact member `key` names an attribute the filters read.
    ///
    /// A key that does not expand names no attribute the filters could have
    /// named, so it is not read.
    async fn reads(&mut self, key: &str) -> bool {
        if let Some(decided) = self.decided.get(key) {
            return *decided;
        }
        let reads = match self.expander.expand_term(key).await {
            Ok(expanded) => self.read.contains(expanded.as_str()),
            Err(_) => false,
        };
        self.decided.insert(key.to_owned(), reads);
        reads
    }
}

#[cfg(test)]
mod tests {
    use crate::answer_narrowing::AnswerNarrowing;
    use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine, term_expansion::ContextualExpander};
    use andromeda_ngsi_ld_query::{entity_selection::EntitySelection, scope::query::ScopeQuery};
    use serde_json::{Value, json};

    /// The answer narrowed for a `q` over `says` and, when `scope` is given, a
    /// scope query.
    async fn narrowed(scope: Option<&str>, answer: &Value) -> Value {
        let engine = JsonLdEngine::builder().allow_remote(false).build().expect("build engine");
        let context = ContextSpec::from_body(json!({"says": "https://example.com/says", "speed": "https://example.com/speed"}));
        let prepared = engine.prepare_term_context(&context);
        let expander = ContextualExpander::new(&engine, &prepared);
        let selection = EntitySelection::builder()
            .q_expr(Some("\"https://example.com/says\"==\"yes\"".parse().expect("a valid q")))
            .scope_q(scope.map(|raw| ScopeQuery::parse(raw).expect("a valid scopeQ")))
            .build();
        AnswerNarrowing::new(&selection, &expander).narrow(answer).await
    }

    /// The identifying members and the attributes the filters read are kept as
    /// the source wrote them, `temporalValues` pairs included; the members no
    /// filter reads are left out.
    #[tokio::test]
    async fn the_members_the_filters_read_are_kept_as_written() {
        let answer = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "says": {"type": "LanguageProperty", "languageMaps": [[{"languageMap": {"en": "yes"}}, "2020-08-01T12:00:00Z"]]},
            "speed": {"type": "Property", "values": [[60, "2020-08-01T12:00:00Z"]]},
            "scope": "/Madrid"
        });
        pretty_assertions::assert_eq!(
            narrowed(None, &answer).await,
            json!({
                "id": "urn:ngsi-ld:Vehicle:1",
                "type": "Vehicle",
                "says": {"type": "LanguageProperty", "languageMaps": [[{"languageMap": {"en": "yes"}}, "2020-08-01T12:00:00Z"]]}
            })
        );
        pretty_assertions::assert_eq!(narrowed(Some("/Madrid"), &answer).await["scope"], json!("/Madrid"));
    }
}
