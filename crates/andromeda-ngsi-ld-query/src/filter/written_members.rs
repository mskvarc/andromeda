//! The written spelling of a filter's trailing `[…]` members, carried back
//! onto an expanded filter read from its persisted form (clause 4.9).
//!
//! Expanding a filter rewrites a compound member to the IRI it addresses in a
//! stored structured value, while a `LanguageProperty`'s target language and a
//! raw JSON key are read in the spelling the client wrote. The expanded filter
//! holds both, and its clause 4.9 text holds only the first; the text
//! [`serialize_written_members`](crate::filter::serialize::serialize_written_members)
//! renders holds the second, and this module joins the two again.

use crate::filter::{expression::Expr, path::AttrPath};

impl Expr {
    /// This expression, each trailing member's written spelling read from
    /// `written`: the same filter rendered by
    /// [`serialize_written_members`](crate::filter::serialize::serialize_written_members)
    /// and parsed back.
    ///
    /// The two are one filter in two spellings, so their Query Terms pair up in
    /// order. A pair whose members do not line up keeps its own spelling, as
    /// does every term of an expression whose term count `written` does not
    /// share.
    #[must_use]
    pub fn with_written_members(mut self, written: &Expr) -> Expr {
        let written_paths = terminal_paths(written);
        let mut own_paths = terminal_paths_mut(&mut self);
        if own_paths.len() == written_paths.len() {
            for (own, written) in own_paths.iter_mut().zip(written_paths) {
                if own.value_path.len() == written.value_path.len() {
                    own.value_path_compact.clone_from(&written.value_path);
                }
            }
        }
        self
    }
}

/// The local path every Query Term of `expr` terminates at, in pre-order.
fn terminal_paths(expr: &Expr) -> Vec<&AttrPath> {
    match expr {
        Expr::And(terms) | Expr::Or(terms) => terms.iter().flat_map(terminal_paths).collect(),
        Expr::Pred(pred) => vec![pred.path().terminal()],
    }
}

/// The local path every Query Term of `expr` terminates at, mutably, in
/// pre-order.
fn terminal_paths_mut(expr: &mut Expr) -> Vec<&mut AttrPath> {
    match expr {
        Expr::And(terms) | Expr::Or(terms) => terms.iter_mut().flat_map(terminal_paths_mut).collect(),
        Expr::Pred(pred) => vec![pred.path_mut().terminal_mut()],
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::QueryError,
        filter::{
            eval::evaluate,
            expansion::expand_q_expr,
            expression::Expr,
            json_key_attributes::JsonKeyAttributes,
            parser::parse,
            serialize::{serialize, serialize_written_members},
        },
        queried_entity::{expanded_fixture::FixtureEntity, linked_entities::NoLinkedEntities},
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
        assert!(evaluate(&english, &door, NoLinkedEntities));
        let addressed_only: Expr = serialize(&english).parse().expect("parses");
        assert!(!evaluate(&addressed_only, &door, NoLinkedEntities), "the expanded text alone has lost the tag");
    }

    /// The compound reading still addresses the expanded member, and every term
    /// of a boolean expression gets its own members back.
    #[tokio::test]
    async fn every_term_keeps_its_members_in_both_spellings() {
        let expr = persisted_round_trip("speed>1;(address[city]==\"Berlin\"|name[en]==\"Door\")").await;
        let rendered = (serialize(&expr), serialize_written_members(&expr));
        pretty_assertions::assert_eq!(
            rendered,
            (
                "\"https://example.com/speed\">1;(\"https://example.com/address\"[\"https://example.com/city\"]==\"Berlin\"|\"https://example.com/name\"[\"https://example.com/en\"]==\"Door\")".to_owned(),
                "\"https://example.com/speed\">1;(\"https://example.com/address\"[city]==\"Berlin\"|\"https://example.com/name\"[en]==\"Door\")".to_owned(),
            )
        );
    }

    /// A written text naming other terms leaves the expression as it is.
    #[test]
    fn a_written_text_of_another_shape_changes_nothing() {
        let expanded = parse("\"https://example.com/name\"[\"https://example.com/en\"]==\"Door\"").expect("parses");
        let other = parse("a==1;b==2").expect("parses");
        pretty_assertions::assert_eq!(
            serialize_written_members(&expanded.clone().with_written_members(&other)),
            serialize_written_members(&expanded)
        ); // clone: the expression is compared before and after
    }
}
