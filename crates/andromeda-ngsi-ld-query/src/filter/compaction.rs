//! Term compaction of an expanded `q` filter (clause 4.9, clause 5.5.7).
//!
//! The inverse of [`crate::filter::expansion`]: a stored filter carries the
//! expanded IRIs it was written with, and a reader receives it spelled in the
//! terms of the `@context` the read supplied. Every position expansion
//! rewrote — the attribute, its sub-attributes, the compound-value members, a
//! hop's Relationship and its entity-type guard — is rewritten back here, and
//! nothing else is touched: operators, operands and the boolean structure
//! come back exactly as stored.
//!
//! As with expansion, the walker lives beside the AST it rewrites and asks a
//! [`TermCompactor`] for the one thing it needs from JSON-LD.

use crate::{
    filter::{
        expression::Expr,
        path::{AttrPath, QueryPath},
        predicate::Predicate,
    },
    term_compactor::TermCompactor,
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use std::{future::Future, pin::Pin};

/// Recursively compact every attribute term of a `q` expression.
///
/// The boolean structure, the operators and the operands survive unchanged,
/// so `expand_q_expr` applied to the result under the same `@context` yields
/// the expression this was given.
pub async fn compact_q_expr(expr: Expr, compactor: &impl TermCompactor) -> Expr {
    match expr {
        Expr::And(terms) => Expr::And(compact_all_terms(terms, compactor).await),
        Expr::Or(terms) => Expr::Or(compact_all_terms(terms, compactor).await),
        Expr::Pred(pred) => Expr::Pred(compact_pred_terms(pred, compactor).await),
    }
}

/// Compact the operands of one boolean node in order.
async fn compact_all_terms(terms: Vec<Expr>, compactor: &impl TermCompactor) -> Vec<Expr> {
    let mut compacted = Vec::with_capacity(terms.len());
    for term in terms {
        compacted.push(compact_q_expr_boxed(term, compactor).await);
    }
    compacted
}

/// Box-pinned wrapper: the walker recurses, and an `async fn` cannot name its
/// own future type.
fn compact_q_expr_boxed<'a, C: TermCompactor>(expr: Expr, compactor: &'a C) -> Pin<Box<dyn Future<Output = Expr> + Send + 'a>> {
    Box::pin(compact_q_expr(expr, compactor))
}

/// Compact the attribute axis of a single predicate, keeping its operator and
/// operand.
async fn compact_pred_terms(pred: Predicate, compactor: &impl TermCompactor) -> Predicate {
    match pred {
        Predicate::Exists(path) => Predicate::Exists(compact_query_path(path, compactor).await),
        Predicate::Equality { path, membership, rhs } => Predicate::Equality {
            path: compact_query_path(path, compactor).await,
            membership,
            rhs,
        },
        Predicate::Ordering { path, op, rhs } => Predicate::Ordering {
            path: compact_query_path(path, compactor).await,
            op,
            rhs,
        },
        Predicate::MatchPattern { path, pattern } => Predicate::MatchPattern {
            path: compact_query_path(path, compactor).await,
            pattern,
        },
        Predicate::NotMatchPattern { path, pattern } => Predicate::NotMatchPattern {
            path: compact_query_path(path, compactor).await,
            pattern,
        },
    }
}

/// Compact an attribute axis: every local segment as a term, and for each
/// linked-entity hop the Relationship as a term and its guard as entity types
/// — the same split [`crate::filter::expansion::expand_query_path`] makes in
/// the other direction.
pub async fn compact_query_path(path: QueryPath, compactor: &impl TermCompactor) -> QueryPath {
    match path {
        QueryPath::Local(local) => QueryPath::Local(compact_attr_path(local, compactor).await),
        QueryPath::Linked {
            relationship,
            guard_types,
            inner,
        } => {
            let relationship = compactor.compact_term(&relationship).await;
            let mut compacted_guard = Vec::with_capacity(guard_types.len());
            for guard_type in &guard_types {
                compacted_guard.push(compactor.compact_type(guard_type).await);
            }
            let inner = compact_query_path_boxed(*inner, compactor).await;
            QueryPath::Linked {
                relationship,
                guard_types: compacted_guard,
                inner: Box::new(inner),
            }
        }
    }
}

/// Box-pinned wrapper: the axis walker recurses through hops, and an
/// `async fn` cannot name its own future type.
fn compact_query_path_boxed<'a, C: TermCompactor>(path: QueryPath, compactor: &'a C) -> Pin<Box<dyn Future<Output = QueryPath> + Send + 'a>> {
    Box::pin(compact_query_path(path, compactor))
}

/// Compact an attribute path — main attribute, sub-attributes and
/// compound-value member names — to terms.
///
/// The compacted member names become the path's compact spelling as well:
/// a freshly parsed path carries what the client wrote in both `value_path`
/// and `value_path_compact`, and a compacted path is meant to read as one.
pub async fn compact_attr_path(mut path: AttrPath, compactor: &impl TermCompactor) -> AttrPath {
    path.attr = compactor.compact_term(&path.attr).await;
    path.sub_attrs = compact_all(&path.sub_attrs, compactor).await;
    path.value_path = compact_all(&path.value_path, compactor).await;
    path.value_path_compact = path.value_path.clone(); // clone: the compact spelling is the compacted one
    path
}

/// Compact every name in a list.
async fn compact_all(names: &[AttributeName], compactor: &impl TermCompactor) -> Vec<AttributeName> {
    let mut compacted = Vec::with_capacity(names.len());
    for name in names {
        compacted.push(compactor.compact_term(name).await);
    }
    compacted
}

#[cfg(test)]
mod tests {
    use crate::{
        error::QueryError,
        filter::{compaction::compact_q_expr, expansion::expand_q_expr, parser, serialize::serialize},
        term_compactor::TermCompactor,
        term_expander::TermExpander,
    };
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use iri_rs::IriBuf;
    use std::future::{Future, ready};

    const TERM_PREFIX: &str = "https://example.com/";
    const TYPE_PREFIX: &str = "https://example.com/types/";

    /// An expander that prefixes every term, paired below with the compactor
    /// that strips the same prefix, so the pair is an exact inverse.
    struct PrefixExpander;

    impl TermExpander for PrefixExpander {
        fn expand_term(&self, term: &str) -> impl Future<Output = Result<IriBuf, QueryError>> + Send {
            ready(Ok(IriBuf::new(format!("{TERM_PREFIX}{term}")).expect("valid IRI")))
        }

        fn expand_type(&self, term: &str) -> impl Future<Output = Result<IriBuf, QueryError>> + Send {
            ready(Ok(IriBuf::new(format!("{TYPE_PREFIX}{term}")).expect("valid IRI")))
        }
    }

    /// The inverse of [`PrefixExpander`]: a name carrying the prefix loses it,
    /// any other name is answered as given, which is the clause 4.5.1 rule an
    /// implementation follows for an IRI no term abbreviates.
    struct PrefixCompactor;

    fn strip(name: &AttributeName, prefix: &str) -> AttributeName {
        match name.as_str().strip_prefix(prefix) {
            Some(term) => term.parse().expect("a stripped term is a valid attribute name"),
            None => name.clone(),
        }
    }

    impl TermCompactor for PrefixCompactor {
        fn compact_term(&self, name: &AttributeName) -> impl Future<Output = AttributeName> + Send {
            ready(strip(name, TERM_PREFIX))
        }

        fn compact_type(&self, name: &AttributeName) -> impl Future<Output = AttributeName> + Send {
            ready(strip(name, TYPE_PREFIX))
        }
    }

    /// The suite's operators and path shapes: every operand kind, every
    /// operator, the three local axes, a guarded and an unguarded hop, and the
    /// boolean structure with an explicit group.
    const SHAPES: [&str; 18] = [
        "speed>50",
        "speed>=50",
        "speed<50",
        "speed<=50",
        "speed==10..20",
        "speed!=10,20,30",
        "active==true",
        "colour==\"red\",\"blue\"",
        "observed>=2017-12-24T12:00:00Z",
        "moment==\"12:00:00Z\"..\"13:00:00Z\"",
        "name~=\"^A.*\"",
        "name!~=\"^A.*\"",
        "speed",
        "temperature.observedAt<2024-01-01T00:00:00Z",
        "address[city]==\"Berlin\"",
        "sensor.rawdata[airquality.particulate]==40",
        "isParkedIn{Building,Store:category==\"x\"}",
        "(a|b);c{d{e==5}}",
    ];

    /// Clause 5.5.7 in both directions: a filter expanded when the resource
    /// was written and compacted when it is read must come back as the filter
    /// the client wrote, for every operator and path shape the grammar has.
    #[tokio::test]
    async fn compacting_an_expanded_filter_restores_what_the_client_wrote() {
        for shape in SHAPES {
            let parsed = parser::parse(shape).unwrap_or_else(|error| panic!("{shape} must parse: {error}"));
            let expanded = expand_q_expr(parsed.clone(), &PrefixExpander).await.expect("expands");
            assert_ne!(serialize(&expanded), shape, "the expansion of {shape} must differ from it");

            let compacted = compact_q_expr(expanded, &PrefixCompactor).await;

            pretty_assertions::assert_eq!(compacted, parsed, "compaction of the expansion of {shape}");
            pretty_assertions::assert_eq!(serialize(&compacted), shape, "serialisation after compaction of {shape}");
        }
    }

    /// The other composition order: expanding a compacted filter yields the
    /// expanded filter again, so a stored filter survives a read-back and a
    /// re-write unchanged.
    #[tokio::test]
    async fn expanding_a_compacted_filter_restores_the_stored_expansion() {
        for shape in SHAPES {
            let parsed = parser::parse(shape).unwrap_or_else(|error| panic!("{shape} must parse: {error}"));
            let expanded = expand_q_expr(parsed, &PrefixExpander).await.expect("expands");

            let compacted = compact_q_expr(expanded.clone(), &PrefixCompactor).await;
            let re_expanded = expand_q_expr(compacted, &PrefixExpander).await.expect("expands again");

            pretty_assertions::assert_eq!(re_expanded, expanded, "expand after compact of {shape}");
        }
    }

    /// An IRI the context does not abbreviate stays a full IRI (clause 4.5.1),
    /// and the operator and operand beside it are untouched.
    #[tokio::test]
    async fn a_name_no_term_abbreviates_is_kept_as_the_full_iri() {
        let expanded = parser::parse("\"https://other.example/speed\">50;\"https://example.com/colour\"==\"red\"").expect("parses");

        let compacted = compact_q_expr(expanded, &PrefixCompactor).await;

        pretty_assertions::assert_eq!(serialize(&compacted), "\"https://other.example/speed\">50;colour==\"red\"");
    }

    /// A hop's two halves compact through different vocabularies, the mirror
    /// of expansion: the Relationship is an attribute term, the guard names
    /// entity types.
    #[tokio::test]
    async fn a_hop_compacts_its_relationship_as_a_term_and_its_guard_as_entity_types() {
        let expanded = parser::parse(
            "\"https://example.com/isParkedIn\"{\"https://example.com/types/Building\",\"https://example.com/types/Store\":\"https://example.com/category\"==\"x\"}",
        )
        .expect("parses");

        let compacted = compact_q_expr(expanded, &PrefixCompactor).await;

        pretty_assertions::assert_eq!(serialize(&compacted), "isParkedIn{Building,Store:category==\"x\"}");
    }
}
