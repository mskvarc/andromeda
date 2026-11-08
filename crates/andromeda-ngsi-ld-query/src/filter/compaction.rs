//! Term compaction of an expanded `q` filter (clause 4.9, clause 5.5.7).
//!
//! The inverse of [`crate::filter::expansion`]: a stored filter carries the
//! expanded IRIs it was written with, and a reader receives it spelled in the
//! terms of the `@context` the read supplied. Every position expansion
//! rewrote (the attribute, its sub-attributes, the compound-value members, a
//! hop's Relationship and its entity-type guard) is rewritten back here, and
//! nothing else is touched: operators, operands and the boolean structure
//! come back exactly as stored.
//!
//! As with expansion, the walker lives beside the AST it rewrites and asks a
//! [`TermCompactor`] for the one thing it needs from JSON-LD.

use crate::{
    filter::{
        expression::Expr,
        json_key_attributes::JsonKeyAttributes,
        path::{AttrPath, QueryPath},
        predicate::Predicate,
        scalar::{CompareValue, Scalar},
        trailing_members::TrailingMembers,
        trailing_path::TrailingPath,
    },
    term_compactor::TermCompactor,
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use iri_rs::Iri;
use std::{collections::HashSet, future::Future, hash::BuildHasher, pin::Pin};

/// Recursively compact every attribute term of a `q` expression.
///
/// The boolean structure, the operators and the operands survive unchanged,
/// so `expand_q_expr` applied to the result under the same `@context` yields
/// the expression this was given.
pub async fn compact_q_expr(expr: Expr, compactor: &impl TermCompactor) -> Expr {
    compact_expr(expr, TrailingMembers::Terms(&JsonKeyAttributes::default()), compactor).await
}

/// Recursively compact every property term of a `csf` expression, leaving its
/// compound-value members as written.
///
/// The inverse of [`crate::filter::expansion::expand_csf_expr`]: the members
/// address raw JSON keys (see [`TrailingMembers::Verbatim`]), which no
/// `@context` abbreviates.
pub async fn compact_csf_expr(expr: Expr, compactor: &impl TermCompactor) -> Expr {
    compact_expr(expr, TrailingMembers::Verbatim, compactor).await
}

/// Compact every attribute term of an expression whose trailing members are
/// spelled as `members` says.
async fn compact_expr(expr: Expr, members: TrailingMembers<'_>, compactor: &impl TermCompactor) -> Expr {
    match expr {
        Expr::And(terms) => Expr::And(compact_all_terms(terms, members, compactor).await),
        Expr::Or(terms) => Expr::Or(compact_all_terms(terms, members, compactor).await),
        Expr::Pred(pred) => Expr::Pred(compact_pred_terms(pred, members, compactor).await),
    }
}

/// Undo the `expandValues` coercion of clause 5.2.12 on a stored filter.
///
/// Table 5.2.12-1 defines `expandValues` as a query-execution instruction:
/// the values of the named attributes "should be expanded against the supplied
/// @context using JSON-LD type coercion **prior to executing the query**", so
/// the coercion belongs to the filter a match runs, not to the filter the
/// client wrote. A Subscription echoes the one the client wrote, and this is
/// what puts the operands back the way they arrived: the exact inverse of
/// [`crate::filter::expansion::apply_expand_values`], over exactly the
/// attributes that opted in.
///
/// `expand_attrs` is the stored, expanded opt-in list, and the paths it is
/// matched against are the stored, expanded ones, so this runs before
/// [`compact_q_expr`] turns those paths back into terms.
pub async fn revert_expand_values<S: BuildHasher + Sync>(expr: Expr, expand_attrs: &HashSet<AttributeName, S>, compactor: &impl TermCompactor) -> Expr {
    match expr {
        Expr::And(terms) => Expr::And(revert_all_terms(terms, expand_attrs, compactor).await),
        Expr::Or(terms) => Expr::Or(revert_all_terms(terms, expand_attrs, compactor).await),
        Expr::Pred(pred) => Expr::Pred(revert_compare_values(pred, expand_attrs, compactor).await),
    }
}

/// Revert the operands of one boolean node in order.
async fn revert_all_terms<S: BuildHasher + Sync>(terms: Vec<Expr>, expand_attrs: &HashSet<AttributeName, S>, compactor: &impl TermCompactor) -> Vec<Expr> {
    let mut reverted = Vec::with_capacity(terms.len());
    for term in terms {
        reverted.push(revert_expand_values_boxed(term, expand_attrs, compactor).await);
    }
    reverted
}

/// Box-pinned wrapper: the walker recurses, and an `async fn` cannot name its
/// own future type.
fn revert_expand_values_boxed<'a, S: BuildHasher + Sync, C: TermCompactor>(
    expr: Expr,
    expand_attrs: &'a HashSet<AttributeName, S>,
    compactor: &'a C,
) -> Pin<Box<dyn Future<Output = Expr> + Send + 'a>> {
    Box::pin(revert_expand_values(expr, expand_attrs, compactor))
}

/// Put the right-hand side of a comparison back in the client's terms when its
/// attribute opted in via `expandValues`.
async fn revert_compare_values<S: BuildHasher + Sync>(pred: Predicate, expand_attrs: &HashSet<AttributeName, S>, compactor: &impl TermCompactor) -> Predicate {
    match pred {
        Predicate::Equality { path, membership, rhs } if expand_attrs.contains(path.terminal().attr.as_str()) => Predicate::Equality {
            rhs: revert_rhs_values(rhs, compactor).await,
            path,
            membership,
        },
        Predicate::Ordering { path, op, rhs } if expand_attrs.contains(path.terminal().attr.as_str()) => Predicate::Ordering {
            rhs: revert_scalar(rhs, compactor).await,
            path,
            op,
        },
        Predicate::Exists(_) | Predicate::Equality { .. } | Predicate::Ordering { .. } | Predicate::MatchPattern { .. } | Predicate::NotMatchPattern { .. } => {
            pred
        }
    }
}

/// Put one coerced scalar operand back in the client's terms.
///
/// Only a string or a `URI` can have been coerced; every other scalar kind was already a
/// value. A string the coercion could not have produced (one that is not a
/// usable attribute name) is the client's own literal and is left alone.
async fn revert_scalar(scalar: Scalar, compactor: &impl TermCompactor) -> Scalar {
    match scalar {
        Scalar::Str(text) => match text.parse::<AttributeName>() {
            Ok(coerced) => Scalar::Str(compactor.compact_term(&coerced).await.as_str().to_owned()),
            Err(_not_a_name) => Scalar::Str(text),
        },
        Scalar::Uri(text) => match text.parse::<AttributeName>() {
            Ok(coerced) => {
                let compact = compactor.compact_term(&coerced).await.as_str().to_owned();
                // A term the compaction leaves is no `URI`, and clause 4.9
                // writes such a value only as a `quotedStr`.
                if Iri::new(compact.as_str()).is_ok() {
                    Scalar::Uri(compact)
                } else {
                    Scalar::Str(compact)
                }
            }
            Err(_not_a_name) => Scalar::Uri(text),
        },
        other @ (Scalar::Number(_) | Scalar::Bool(_) | Scalar::DateTime(_) | Scalar::Date(_) | Scalar::Time(_)) => other,
    }
}

/// Put the string operands of an equality comparison back in the client's
/// terms; a range is bounded by two ordered values, neither of which is a term.
async fn revert_rhs_values(rhs: CompareValue, compactor: &impl TermCompactor) -> CompareValue {
    match rhs {
        CompareValue::Scalar(scalar) => CompareValue::Scalar(revert_scalar(scalar, compactor).await),
        CompareValue::ValueList(values) => {
            let mut reverted = Vec::with_capacity(values.len());
            for value in values {
                reverted.push(revert_scalar(value, compactor).await);
            }
            CompareValue::ValueList(reverted)
        }
        CompareValue::Range { .. } => rhs,
    }
}

/// Compact the operands of one boolean node in order.
async fn compact_all_terms(terms: Vec<Expr>, members: TrailingMembers<'_>, compactor: &impl TermCompactor) -> Vec<Expr> {
    let mut compacted = Vec::with_capacity(terms.len());
    for term in terms {
        compacted.push(compact_expr_boxed(term, members, compactor).await);
    }
    compacted
}

/// Box-pinned wrapper: the walker recurses, and an `async fn` cannot name its
/// own future type.
fn compact_expr_boxed<'a, C: TermCompactor>(expr: Expr, members: TrailingMembers<'a>, compactor: &'a C) -> Pin<Box<dyn Future<Output = Expr> + Send + 'a>> {
    Box::pin(compact_expr(expr, members, compactor))
}

/// Compact the attribute axis of a single predicate, keeping its operator and
/// operand.
async fn compact_pred_terms(pred: Predicate, members: TrailingMembers<'_>, compactor: &impl TermCompactor) -> Predicate {
    match pred {
        Predicate::Exists(path) => Predicate::Exists(compact_query_path(path, members, compactor).await),
        Predicate::Equality { path, membership, rhs } => Predicate::Equality {
            path: compact_query_path(path, members, compactor).await,
            membership,
            rhs,
        },
        Predicate::Ordering { path, op, rhs } => Predicate::Ordering {
            path: compact_query_path(path, members, compactor).await,
            op,
            rhs,
        },
        Predicate::MatchPattern { path, pattern } => Predicate::MatchPattern {
            path: compact_query_path(path, members, compactor).await,
            pattern,
        },
        Predicate::NotMatchPattern { path, pattern } => Predicate::NotMatchPattern {
            path: compact_query_path(path, members, compactor).await,
            pattern,
        },
    }
}

/// Compact an attribute axis: every local segment as a term, and for each
/// linked-entity hop the Relationship as a term and its guard as entity types.
/// This is the same split [`crate::filter::expansion::expand_query_path`] makes
/// in the other direction.
pub async fn compact_query_path(path: QueryPath, members: TrailingMembers<'_>, compactor: &impl TermCompactor) -> QueryPath {
    match path {
        QueryPath::Local(local) => QueryPath::Local(compact_attr_path(local, members, compactor).await),
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
            let inner = compact_query_path_boxed(*inner, members, compactor).await;
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
fn compact_query_path_boxed<'a, C: TermCompactor>(
    path: QueryPath,
    members: TrailingMembers<'a>,
    compactor: &'a C,
) -> Pin<Box<dyn Future<Output = QueryPath> + Send + 'a>> {
    Box::pin(compact_query_path(path, members, compactor))
}

/// Compact an attribute path (main attribute, sub-attributes and, when
/// `members` makes them terms, compound-value member names) to terms.
///
/// The member names become the path's written spelling as well: a freshly
/// parsed path carries what the client wrote in both spellings of its
/// [`MemberPath`], and a compacted path is meant to read as one. A target
/// language is no term and stays as written.
pub async fn compact_attr_path(mut path: AttrPath, members: TrailingMembers<'_>, compactor: &impl TermCompactor) -> AttrPath {
    if let Some(TrailingPath::Members(member_path)) = &mut path.trailing {
        if members.are_terms_under(&path.attr) {
            member_path.addressed = compact_all(&member_path.addressed, compactor).await;
        }
        member_path.written.clone_from(&member_path.addressed); // clone: the written and addressed spellings are separate owned values
    }
    path.attr = compactor.compact_term(&path.attr).await;
    path.sub_attrs = compact_all(&path.sub_attrs, compactor).await;
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
        filter::{
            compaction::{compact_csf_expr, compact_q_expr, revert_expand_values},
            expansion::{apply_expand_values, expand_csf_expr, expand_q_expr},
            json_key_attributes::JsonKeyAttributes,
            parser,
            serialize::serialize,
        },
        term_compactor::TermCompactor,
        term_expander::TermExpander,
    };
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use iri_rs::IriBuf;
    use std::{
        collections::HashSet,
        future::{Future, ready},
    };

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
        "isParkedIn{Building,Store:category}==\"x\"",
        "(a|b);c{d{e}}==5",
    ];

    /// Clause 5.5.7 in both directions: a filter expanded when the resource
    /// was written and compacted when it is read must come back as the filter
    /// the client wrote, for every operator and path shape the grammar has.
    #[tokio::test]
    async fn compacting_an_expanded_filter_restores_what_the_client_wrote() {
        for shape in SHAPES {
            let parsed = parser::parse(shape).unwrap_or_else(|error| panic!("{shape} must parse: {error}"));
            let expanded = expand_q_expr(parsed.clone(), &JsonKeyAttributes::default(), &PrefixExpander)
                .await
                .expect("expands");
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
            let expanded = expand_q_expr(parsed, &JsonKeyAttributes::default(), &PrefixExpander).await.expect("expands");

            let compacted = compact_q_expr(expanded.clone(), &PrefixCompactor).await;
            let re_expanded = expand_q_expr(compacted, &JsonKeyAttributes::default(), &PrefixExpander)
                .await
                .expect("expands again");

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
            "\"https://example.com/isParkedIn\"{\"https://example.com/types/Building\",\"https://example.com/types/Store\":\"https://example.com/category\"}==\"x\"",
        )
        .expect("parses");

        let compacted = compact_q_expr(expanded, &PrefixCompactor).await;

        pretty_assertions::assert_eq!(serialize(&compacted), "isParkedIn{Building,Store:category}==\"x\"");
    }

    /// A `csf` round-trips with its trailing members exactly as written, even
    /// a member that reads like a term the compactor would abbreviate: those
    /// members are the raw JSON keys of a Context Source Property (clause 4.9).
    #[tokio::test]
    async fn a_csf_round_trips_with_its_trailing_members_as_written() {
        let written = "coverage.since[\"https://example.com/area\".region]==\"north\"";
        let parsed = parser::parse(written).expect("parses");
        let expanded = expand_csf_expr(parsed.clone(), &PrefixExpander).await.expect("expands");

        let compacted = compact_csf_expr(expanded.clone(), &PrefixCompactor).await;

        pretty_assertions::assert_eq!(compacted, parsed);
        pretty_assertions::assert_ne!(
            compact_q_expr(expanded, &PrefixCompactor).await,
            parsed,
            "an Entity filter compacts the same member as a term"
        );
    }

    /// A quoted word `expandValues` expanded reads back quoted, as the client
    /// wrote it, so a stored filter is answered in the form it was written in.
    #[tokio::test]
    async fn an_expanded_quoted_word_reverts_to_the_quoted_word() {
        let written = "gender==\"Male\"";
        let parsed = parser::parse(written).expect("parses");
        let expanded = expand_q_expr(parsed, &JsonKeyAttributes::default(), &PrefixExpander).await.expect("expands");
        let opted_in: HashSet<AttributeName> = HashSet::from([format!("{TERM_PREFIX}gender").parse().expect("a valid name")]);
        let coerced = apply_expand_values(expanded, &opted_in, &PrefixExpander).await.expect("expands");

        let reverted = compact_q_expr(revert_expand_values(coerced, &opted_in, &PrefixCompactor).await, &PrefixCompactor).await;

        pretty_assertions::assert_eq!(serialize(&reverted), written);
    }
}
