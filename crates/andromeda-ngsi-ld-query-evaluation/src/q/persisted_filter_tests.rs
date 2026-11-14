use crate::{
    q::eval::evaluate,
    queried_entity::{expanded_fixture::FixtureEntity, linked_entities::NoLinkedEntities},
};
use andromeda_ngsi_ld_query::{
    error::QueryError,
    filter::{
        expansion::expand_q_expr,
        expression::Expr,
        json_key_attributes::JsonKeyAttributes,
        language_matching::LanguageMatching,
        parser::parse,
        serialize::{serialize, serialize_written_members},
    },
    term_expander::TermExpander,
};
use iri_rs::IriBuf;
use serde_json::json;
use std::future::{Future, ready};

/// Expands every term under `https://example.com/`, as a `@vocab` would.
struct VocabExpander;

impl TermExpander for VocabExpander {
    fn expand_term(&self, term: &str) -> impl Future<Output = Result<IriBuf, QueryError>> + Send {
        ready(Ok(IriBuf::new(format!("https://example.com/{term}")).expect("an IRI")))
    }

    fn expand_type(&self, term: &str) -> impl Future<Output = Result<IriBuf, QueryError>> + Send {
        self.expand_term(term)
    }
}

/// The filter as a store reads it back: the expanded text, joined with the
/// written spelling of its members.
async fn persisted_round_trip(q: &str) -> Expr {
    let expanded = expand_q_expr(parse(q).expect("parses"), &JsonKeyAttributes::default(), &VocabExpander)
        .await
        .expect("expands");
    let addressed: Expr = serialize(&expanded).parse().expect("the expanded text parses");
    let written: Expr = serialize_written_members(&expanded).parse().expect("the written text parses");
    addressed.with_written_members(&written)
}

/// Clause 4.9: a persisted `name[en]` still reads the English value of a
/// `LanguageProperty`, which the expanded text alone spells
/// `name["https://example.com/en"]`.
#[tokio::test]
async fn a_persisted_target_language_keeps_the_written_tag() {
    let door = FixtureEntity::from_expanded(&json!({
        "@id": "urn:ngsi-ld:Door:1",
        "@type": ["https://example.com/Door"],
        "https://example.com/name": [{
            "https://uri.etsi.org/ngsi-ld/hasLanguageMap": [
                {"@language": "en", "@value": "Door"},
                {"@language": "de", "@value": "Tuer"}
            ]
        }]
    }));
    let english = persisted_round_trip("name[en]==\"Door\"").await;
    assert!(evaluate(&english, &door, NoLinkedEntities, LanguageMatching::TagEquality));
    let addressed_only: Expr = serialize(&english).parse().expect("parses");
    assert!(
        !evaluate(&addressed_only, &door, NoLinkedEntities, LanguageMatching::TagEquality),
        "the expanded text alone has lost the tag"
    );
}
