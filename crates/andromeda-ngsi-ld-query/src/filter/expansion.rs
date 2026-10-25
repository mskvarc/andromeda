//! Term expansion of a parsed `q` filter (clause 4.9, clause 6.4.3.2).
//!
//! A filter arrives with compact attribute names and, for `expandValues` and
//! `jsonKeys`, compact values and object keys too. Each of those becomes a full
//! IRI here, before anything downstream tries to address a stored attribute
//! with it.
//!
//! These walkers live beside the AST they rewrite rather than in the JSON-LD
//! layer. They are shaped entirely by `Expr`, `Predicate`, `AttrPath` and
//! `Scalar`; the only thing they need from JSON-LD is a
//! [`TermExpander`].

#[cfg(test)]
use crate::filter::parser;
use crate::{
    error::QueryError,
    filter::{
        expression::Expr,
        json_key_attributes::JsonKeyAttributes,
        path::{AttrPath, QueryPath, expanded_attribute_name},
        predicate::Predicate,
        scalar::{CompareValue, Scalar},
        trailing_members::TrailingMembers,
        trailing_path::TrailingPath,
    },
    term_expander::TermExpander,
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use std::{collections::HashSet, future::Future, hash::BuildHasher, pin::Pin};

/// Parse and expand a `q` filter expression, as the tests of this crate state
/// one.
///
/// Returns `None` when `q` is absent or empty.
///
/// # Errors
///
/// Returns a `Q*` variant of [`QueryError`] when the expression is outside the
/// clause 4.9 grammar, and [`QueryError::TermExpansion`],
/// [`QueryError::TermNotAnIri`] or [`QueryError::AttributeNameInvalid`] when a
/// term does not expand to a usable attribute name.
#[cfg(test)]
pub(crate) async fn expand_q_expression(q: Option<&str>, json_keys: &JsonKeyAttributes, expander: &impl TermExpander) -> Result<Option<Expr>, QueryError> {
    let Some(q_str) = q.map(str::trim).filter(|trimmed| !trimmed.is_empty()) else {
        return Ok(None);
    };
    let expr = parser::parse(q_str)?;
    Ok(Some(expand_q_expr(expr, json_keys, expander).await?))
}

/// Recursively expand attribute terms in a `q` expression.
///
/// Every attribute path (main attribute plus sub-attributes) in the expression
/// is expanded to its full IRI form, and so are the compound-value member names
/// of every attribute `json_keys` did not declare to hold raw JSON.
///
/// # Errors
///
/// Returns [`QueryError::TermExpansion`], [`QueryError::TermNotAnIri`] or
/// [`QueryError::AttributeNameInvalid`] when a term does not expand to a usable
/// attribute name.
pub async fn expand_q_expr(expr: Expr, json_keys: &JsonKeyAttributes, expander: &impl TermExpander) -> Result<Expr, QueryError> {
    let members = TrailingMembers::Terms(json_keys);
    walk_predicates(expr, &|pred| expand_pred_terms(pred, members, expander)).await
}

/// Recursively expand the property terms of a `csf` expression.
///
/// Every attribute path's property and sub-properties expand to full IRIs, as
/// a registration's Context Source Properties are held under their expanded
/// names (clause 5.5.7); the compound-value members never do, because those
/// values are raw JSON (see [`TrailingMembers::Verbatim`]).
///
/// # Errors
///
/// Returns [`QueryError::TermExpansion`], [`QueryError::TermNotAnIri`] or
/// [`QueryError::AttributeNameInvalid`] when a term does not expand to a usable
/// attribute name.
pub async fn expand_csf_expr(expr: Expr, expander: &impl TermExpander) -> Result<Expr, QueryError> {
    walk_predicates(expr, &|pred| expand_pred_terms(pred, TrailingMembers::Verbatim, expander)).await
}

/// Apply `expandValues`: for attributes in `expand_attrs`, expand string scalar
/// right-hand-side values through the `@context` (clause 6.4.3.2).
///
/// # Errors
///
/// Returns [`QueryError::TermExpansion`] or [`QueryError::TermNotAnIri`] when a
/// string operand of an opted-in attribute does not expand.
pub async fn apply_expand_values<S: BuildHasher + Sync>(
    expr: Expr,
    expand_attrs: &HashSet<AttributeName, S>,
    expander: &impl TermExpander,
) -> Result<Expr, QueryError> {
    walk_predicates(expr, &|pred| expand_compare_values(pred, expand_attrs, expander)).await
}

/// Rewrite every leaf predicate of an expression, leaving its boolean structure
/// intact.
async fn walk_predicates<F, Fut>(expr: Expr, handler: &F) -> Result<Expr, QueryError>
where
    F: Fn(Predicate) -> Fut + Sync,
    Fut: Future<Output = Result<Predicate, QueryError>> + Send,
{
    match expr {
        Expr::And(terms) => {
            let mut expanded = Vec::with_capacity(terms.len());
            for term in terms {
                expanded.push(walk_predicates_boxed(term, handler).await?);
            }
            Ok(Expr::And(expanded))
        }
        Expr::Or(terms) => {
            let mut expanded = Vec::with_capacity(terms.len());
            for term in terms {
                expanded.push(walk_predicates_boxed(term, handler).await?);
            }
            Ok(Expr::Or(expanded))
        }
        Expr::Pred(pred) => Ok(Expr::Pred(handler(pred).await?)),
    }
}

/// Box-pinned wrapper: the walker recurses, and an `async fn` cannot name its
/// own future type.
fn walk_predicates_boxed<'a, F, Fut>(expr: Expr, handler: &'a F) -> Pin<Box<dyn Future<Output = Result<Expr, QueryError>> + Send + 'a>>
where
    F: Fn(Predicate) -> Fut + Sync,
    Fut: Future<Output = Result<Predicate, QueryError>> + Send + 'a,
{
    Box::pin(walk_predicates(expr, handler))
}

/// Expand the attribute path of a single predicate.
async fn expand_pred_terms(pred: Predicate, members: TrailingMembers<'_>, expander: &impl TermExpander) -> Result<Predicate, QueryError> {
    match pred {
        Predicate::Exists(path) => Ok(Predicate::Exists(expand_query_path(path, members, expander).await?)),
        Predicate::Equality { path, membership, rhs } => Ok(Predicate::Equality {
            path: expand_query_path(path, members, expander).await?,
            membership,
            rhs,
        }),
        Predicate::Ordering { path, op, rhs } => Ok(Predicate::Ordering {
            path: expand_query_path(path, members, expander).await?,
            op,
            rhs,
        }),
        Predicate::MatchPattern { path, pattern } => Ok(Predicate::MatchPattern {
            path: expand_query_path(path, members, expander).await?,
            pattern,
        }),
        Predicate::NotMatchPattern { path, pattern } => Ok(Predicate::NotMatchPattern {
            path: expand_query_path(path, members, expander).await?,
            pattern,
        }),
    }
}

/// Expand an attribute axis: every local segment as a term, and for each
/// linked-entity hop the Relationship as a term and its guard as entity types.
///
/// The guard names entity types, not attributes, so it expands through
/// [`TermExpander::expand_type`] — clause 4.9's `EntityType` production, which
/// clause 4.17 expands the same way a `type` selection is expanded.
///
/// # Errors
///
/// Returns [`QueryError::TermExpansion`], [`QueryError::TermNotAnIri`] or
/// [`QueryError::AttributeNameInvalid`] when a segment, a hop's Relationship or
/// one of its guard types does not expand to a usable attribute name.
pub async fn expand_query_path(path: QueryPath, members: TrailingMembers<'_>, expander: &impl TermExpander) -> Result<QueryPath, QueryError> {
    match path {
        QueryPath::Local(local) => Ok(QueryPath::Local(expand_attr_path(local, members, expander).await?)),
        QueryPath::Linked {
            relationship,
            guard_types,
            inner,
        } => {
            let relationship = expanded_attribute_name(&expander.expand_term(relationship.as_str()).await?)?;
            let mut expanded_guard = Vec::with_capacity(guard_types.len());
            for guard_type in &guard_types {
                expanded_guard.push(expanded_attribute_name(&expander.expand_type(guard_type.as_str()).await?)?);
            }
            let inner = expand_query_path_boxed(*inner, members, expander).await?;
            Ok(QueryPath::Linked {
                relationship,
                guard_types: expanded_guard,
                inner: Box::new(inner),
            })
        }
    }
}

/// Box-pinned wrapper: the axis walker recurses through hops, and an `async fn`
/// cannot name its own future type.
fn expand_query_path_boxed<'a, E: TermExpander>(
    path: QueryPath,
    members: TrailingMembers<'a>,
    expander: &'a E,
) -> Pin<Box<dyn Future<Output = Result<QueryPath, QueryError>> + Send + 'a>>
where
    QueryPath: 'a,
{
    Box::pin(expand_query_path(path, members, expander))
}

/// Expand an attribute path — main attribute, sub-attributes and, when
/// `members` makes them terms, compound-value member names — to full IRIs.
///
/// The member names of a compound value expand for the same reason the attribute name does: JSON-LD expansion of a structured Property value turns its members into IRI-keyed nodes, so `address[city]` addresses the expansion of `city` under the request `@context`, not the four letters the client typed.
///
/// # Errors
///
/// Returns [`QueryError::TermExpansion`], [`QueryError::TermNotAnIri`] or
/// [`QueryError::AttributeNameInvalid`] when the attribute, one of its
/// sub-attributes or one of its compound-value members does not expand to a
/// usable attribute name.
pub async fn expand_attr_path(mut path: AttrPath, members: TrailingMembers<'_>, expander: &impl TermExpander) -> Result<AttrPath, QueryError> {
    path.attr = expanded_attribute_name(&expander.expand_term(path.attr.as_str()).await?)?;
    path.sub_attrs = expand_attribute_names(&path.sub_attrs, expander).await?;
    // A value that is raw JSON -- under an attribute the request declared in
    // `jsonKeys` (clause 4.9 EXAMPLE 11), or any Context Source Property -- is
    // addressed by literal object keys, which no `@context` expands.
    if let Some(TrailingPath::Members(member_path)) = &mut path.trailing
        && members.are_terms_under(&path.attr)
    {
        member_path.addressed = expand_attribute_names(&member_path.addressed, expander).await?;
    }
    Ok(path)
}

/// Expand every term of an attribute-name list, in order.
///
/// This is what turns an `expandValues` or `jsonKeys` opt-in list into the
/// names [`apply_expand_values`] and [`JsonKeyAttributes`] key a filter by
/// (clause 4.9): both match against the terminal attribute of an
/// already-expanded predicate path, so a compact opt-in list would key nothing
/// at all. It is also how a path's sub-attributes and compound-value members
/// are expanded.
///
/// # Errors
///
/// Returns [`QueryError::TermExpansion`], [`QueryError::TermNotAnIri`] or
/// [`QueryError::AttributeNameInvalid`] when a term does not expand to a usable
/// attribute name.
pub async fn expand_attribute_names(terms: &[AttributeName], expander: &impl TermExpander) -> Result<Vec<AttributeName>, QueryError> {
    let mut expanded_names = Vec::with_capacity(terms.len());
    for term in terms {
        expanded_names.push(expanded_attribute_name(&expander.expand_term(term.as_str()).await?)?);
    }
    Ok(expanded_names)
}

/// Expand the right-hand side of a comparison when its attribute opted in via
/// `expandValues`.
async fn expand_compare_values<S: BuildHasher + Sync>(
    pred: Predicate,
    expand_attrs: &HashSet<AttributeName, S>,
    expander: &impl TermExpander,
) -> Result<Predicate, QueryError> {
    match pred {
        Predicate::Equality { path, membership, rhs } if expand_attrs.contains(path.terminal().attr.as_str()) => Ok(Predicate::Equality {
            path,
            membership,
            rhs: expand_rhs_values(rhs, expander).await?,
        }),
        Predicate::Ordering { path, op, rhs } if expand_attrs.contains(path.terminal().attr.as_str()) => Ok(Predicate::Ordering {
            path,
            op,
            rhs: expand_scalar(rhs, expander).await?,
        }),
        Predicate::Exists(_) | Predicate::Equality { .. } | Predicate::Ordering { .. } | Predicate::MatchPattern { .. } | Predicate::NotMatchPattern { .. } => {
            Ok(pred)
        }
    }
}

/// Expand a single scalar operand: a string or a `URI` is a term to expand,
/// every other scalar kind is already a value.
async fn expand_scalar(scalar: Scalar, expander: &impl TermExpander) -> Result<Scalar, QueryError> {
    match scalar {
        Scalar::Str(s) => Ok(Scalar::Str(expander.expand_term(&s).await?.to_string())),
        Scalar::Uri(s) => Ok(Scalar::Uri(expander.expand_term(&s).await?.to_string())),
        other @ (Scalar::Number(_) | Scalar::Bool(_) | Scalar::DateTime(_) | Scalar::Date(_) | Scalar::Time(_)) => Ok(other),
    }
}

/// Expand the string operands of an equality comparison; every other scalar
/// kind is already a value, not a term.
async fn expand_rhs_values(rhs: CompareValue, expander: &impl TermExpander) -> Result<CompareValue, QueryError> {
    match rhs {
        CompareValue::Scalar(scalar) => Ok(CompareValue::Scalar(expand_scalar(scalar, expander).await?)),
        CompareValue::ValueList(values) => {
            let mut expanded_values = Vec::with_capacity(values.len());
            for value in values {
                expanded_values.push(expand_scalar(value, expander).await?);
            }
            Ok(CompareValue::ValueList(expanded_values))
        }
        // A range is bounded by two ordered values; neither can be a term.
        CompareValue::Range { .. } => Ok(rhs),
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use super::*;
    use crate::filter::{predicate::OrderingOp, serialize::serialize};
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;
    use std::future::ready;

    /// An expander that prefixes every term, so a test can see exactly which
    /// positions were rewritten without standing up a JSON-LD engine.
    struct PrefixExpander;

    impl TermExpander for PrefixExpander {
        fn expand_term(&self, term: &str) -> impl Future<Output = Result<IriBuf, QueryError>> + Send {
            ready(Ok(IriBuf::new(format!("https://example.com/{term}")).expect("valid IRI")))
        }

        fn expand_type(&self, term: &str) -> impl Future<Output = Result<IriBuf, QueryError>> + Send {
            ready(Ok(IriBuf::new(format!("https://example.com/types/{term}")).expect("valid IRI")))
        }
    }

    fn opted_in(names: &[&str]) -> HashSet<AttributeName> {
        names.iter().map(|name| name.parse().expect("valid attribute name")).collect()
    }

    fn path_of(expr: &Expr) -> &AttrPath {
        match expr {
            Expr::Pred(Predicate::Equality { path, .. } | Predicate::Ordering { path, .. }) => path.terminal(),
            other @ (Expr::And(_) | Expr::Or(_) | Expr::Pred(_)) => panic!("expected a comparison, got {other:?}"),
        }
    }

    fn equality_rhs(expr: &Expr) -> &CompareValue {
        match expr {
            Expr::Pred(Predicate::Equality { rhs, .. }) => rhs,
            other @ (Expr::And(_) | Expr::Or(_) | Expr::Pred(_)) => panic!("expected an equality comparison, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn an_empty_filter_expands_to_nothing() {
        assert!(
            expand_q_expression(None, &JsonKeyAttributes::default(), &PrefixExpander)
                .await
                .expect("no filter")
                .is_none()
        );
        assert!(
            expand_q_expression(Some("   "), &JsonKeyAttributes::default(), &PrefixExpander)
                .await
                .expect("blank filter")
                .is_none()
        );
    }

    #[tokio::test]
    async fn every_path_segment_of_every_predicate_is_expanded() {
        let expanded = expand_q_expression(
            Some("speed>50;temperature.observedAt<2024-01-01T00:00:00Z"),
            &JsonKeyAttributes::default(),
            &PrefixExpander,
        )
        .await
        .expect("expands")
        .expect("present");
        let Expr::And(terms) = expanded else {
            panic!("expected a conjunction");
        };
        let first = path_of(&terms[0]);
        assert_eq!(first.attr.as_str(), "https://example.com/speed");
        let second = path_of(&terms[1]);
        assert_eq!(second.attr.as_str(), "https://example.com/temperature");
        assert_eq!(
            second.sub_attrs.iter().map(AttributeName::as_str).collect::<Vec<_>>(),
            vec!["https://example.com/observedAt"]
        );
    }

    /// A hop's two halves expand through different vocabularies: the
    /// Relationship is an attribute term, the guard names entity types. The stub
    /// expander sends each through a distinguishable prefix so the test can see
    /// that `expand_term` and `expand_type` were applied to the right halves —
    /// swapping them would expand a guard against the attribute vocabulary and
    /// silently stop matching every entity.
    #[tokio::test]
    async fn a_hop_expands_its_relationship_as_a_term_and_its_guard_as_entity_types() {
        let expanded = expand_q_expression(
            Some("isParkedIn{Building,Store:category}==\"x\""),
            &JsonKeyAttributes::default(),
            &PrefixExpander,
        )
        .await
        .expect("expands")
        .expect("present");
        match expanded {
            Expr::Pred(Predicate::Equality { path, .. }) => match path {
                QueryPath::Linked {
                    relationship,
                    guard_types,
                    inner,
                } => {
                    assert_eq!(relationship.as_str(), "https://example.com/isParkedIn");
                    assert_eq!(
                        guard_types.iter().map(AttributeName::as_str).collect::<Vec<&str>>(),
                        vec!["https://example.com/types/Building", "https://example.com/types/Store"]
                    );
                    assert_eq!(inner.terminal().attr.as_str(), "https://example.com/category");
                }
                QueryPath::Local(local) => panic!("expected a hop, got {local:?}"),
            },
            other @ (Expr::And(_) | Expr::Or(_) | Expr::Pred(_)) => panic!("expected an equality predicate, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn nested_hops_are_expanded_at_every_level() {
        let expanded = expand_q_expression(Some("outer{inner{zone}}==\"north\""), &JsonKeyAttributes::default(), &PrefixExpander)
            .await
            .expect("expands")
            .expect("present");
        let rendered = parser::parse(&serialize(&expanded)).expect("reparses");
        assert_eq!(rendered, expanded, "an expanded traversal must round-trip");
        assert_eq!(path_of(&expanded).attr.as_str(), "https://example.com/zone");
    }

    #[tokio::test]
    async fn an_opt_in_list_is_expanded_into_the_names_a_filter_is_keyed_by() {
        let names: Vec<AttributeName> = ["colour", "label"].iter().map(|name| name.parse().expect("valid attribute name")).collect();
        let expanded = expand_attribute_names(&names, &PrefixExpander).await.expect("expands");
        assert_eq!(
            expanded.iter().map(AttributeName::as_str).collect::<Vec<_>>(),
            vec!["https://example.com/colour", "https://example.com/label"]
        );
    }

    /// The opt-in names key predicates whose paths were already expanded, so an
    /// expanded list applied to an expanded filter is what actually rewrites a
    /// value.
    #[tokio::test]
    async fn an_expanded_opt_in_list_keys_an_expanded_filter() {
        let expr = expand_q_expression(Some("colour==\"red\""), &JsonKeyAttributes::default(), &PrefixExpander)
            .await
            .expect("expands")
            .expect("present");
        let names: Vec<AttributeName> = vec!["colour".parse().expect("valid attribute name")];
        let opted_in: HashSet<AttributeName> = expand_attribute_names(&names, &PrefixExpander).await.expect("expands").into_iter().collect();
        let expanded = apply_expand_values(expr, &opted_in, &PrefixExpander).await.expect("expands");
        assert_eq!(
            equality_rhs(&expanded),
            &CompareValue::Scalar(Scalar::Str("https://example.com/red".to_string()))
        );
    }

    #[tokio::test]
    async fn expand_values_rewrites_only_the_named_attributes() {
        let expr = parser::parse("colour==\"red\";label==\"red\"").expect("parses");
        let opted_in = opted_in(&["colour"]);
        let expanded = apply_expand_values(expr, &opted_in, &PrefixExpander).await.expect("expands");
        let Expr::And(terms) = expanded else {
            panic!("expected a conjunction");
        };
        assert_eq!(
            equality_rhs(&terms[0]),
            &CompareValue::Scalar(Scalar::Str("https://example.com/red".to_string()))
        );
        assert_eq!(equality_rhs(&terms[1]), &CompareValue::Scalar(Scalar::Str("red".to_string())));
    }

    #[tokio::test]
    async fn expand_values_leaves_non_string_operands_alone() {
        let expr = parser::parse("speed==10..20").expect("parses");
        let opted_in = opted_in(&["speed"]);
        let expanded = apply_expand_values(expr, &opted_in, &PrefixExpander).await.expect("expands");
        assert_eq!(
            equality_rhs(&expanded),
            &CompareValue::Range {
                min: Scalar::Number(10.into()),
                max: Scalar::Number(20.into()),
            }
        );
    }

    #[tokio::test]
    async fn expand_values_rewrites_the_string_members_of_a_value_list() {
        let expr = parser::parse("colour==\"red\",\"blue\"").expect("parses");
        let opted_in = opted_in(&["colour"]);
        let expanded = apply_expand_values(expr, &opted_in, &PrefixExpander).await.expect("expands");
        assert_eq!(
            equality_rhs(&expanded),
            &CompareValue::ValueList(vec![
                Scalar::Str("https://example.com/red".to_string()),
                Scalar::Str("https://example.com/blue".to_string()),
            ])
        );
    }

    /// Clause 4.9 derives a bare word that is no `URI` from no production, so
    /// it is `BadRequestData` (clause 5.7.2.4) whatever `expandValues` names,
    /// alone, in a value list, under `!=` and inside a logical combination; the
    /// spelling EXAMPLE 12 writes, `gender==Male`, included.
    #[test]
    fn a_bare_word_is_refused_whatever_expand_values_names() {
        for q in [
            "status==active",
            "status==\"on\",active",
            "status!=active",
            "speed>1;(status==active|name==\"x\")",
            "gender==Male",
            "tier==gold",
        ] {
            let error = parser::parse(q).expect_err("a bare word is no value");
            assert!(matches!(error, QueryError::QBareWord), "{q}: {error:?}");
        }
    }

    /// Clause 6.4.3.2 with clause 4.9: a quoted word of an attribute
    /// `expandValues` names expands to the `URI` it names, alone or in a value
    /// list.
    #[tokio::test]
    async fn expand_values_expands_a_quoted_word_to_the_uri_it_names() {
        let opted_in = opted_in(&["gender"]);
        let list = apply_expand_values(parser::parse("gender==\"Male\",\"Female\"").expect("parses"), &opted_in, &PrefixExpander)
            .await
            .expect("expands");
        assert_eq!(
            equality_rhs(&list),
            &CompareValue::ValueList(vec![
                Scalar::Str("https://example.com/Male".to_string()),
                Scalar::Str("https://example.com/Female".to_string()),
            ])
        );
    }

    /// A quoted word and a bare `URI` are values whatever `expandValues` names.
    #[tokio::test]
    async fn a_quoted_word_and_a_bare_uri_stand_without_expand_values() {
        for q in ["status==\"active\"", "ref==urn:ngsi-ld:Device:1", "speed>1"] {
            let expr = parser::parse(q).expect("parses");
            let kept = apply_expand_values(expr.clone(), &HashSet::<AttributeName>::new(), &PrefixExpander)
                .await
                .expect("a value");
            assert_eq!(kept, expr, "{q}");
        }
    }

    /// JSON-LD expansion of a structured Property value turns its members into IRI-keyed nodes, so a compound-value member has to be expanded through the same `@context` the attribute name is.
    #[tokio::test]
    async fn a_compound_value_member_is_expanded_through_the_context() {
        let expanded = expand_q_expression(Some("address[city]==\"Berlin\""), &JsonKeyAttributes::default(), &PrefixExpander)
            .await
            .expect("expands")
            .expect("present");
        let path = path_of(&expanded);
        assert_eq!(path.attr.as_str(), "https://example.com/address");
        assert_eq!(
            path.members()
                .expect("a member path")
                .addressed
                .iter()
                .map(AttributeName::as_str)
                .collect::<Vec<_>>(),
            vec!["https://example.com/city"]
        );
    }

    #[tokio::test]
    async fn every_member_of_a_nested_compound_path_is_expanded() {
        let expanded = expand_q_expression(
            Some("sensor.rawdata[airquality.particulate]==40"),
            &JsonKeyAttributes::default(),
            &PrefixExpander,
        )
        .await
        .expect("expands")
        .expect("present");
        let path = path_of(&expanded);
        assert_eq!(
            path.sub_attrs.iter().map(AttributeName::as_str).collect::<Vec<_>>(),
            vec!["https://example.com/rawdata"]
        );
        assert_eq!(
            path.members()
                .expect("a member path")
                .addressed
                .iter()
                .map(AttributeName::as_str)
                .collect::<Vec<_>>(),
            vec!["https://example.com/airquality", "https://example.com/particulate"]
        );
    }

    #[tokio::test]
    async fn an_expanded_compound_path_round_trips_through_the_serialisation() {
        let expanded = expand_q_expression(Some("address[city]==\"Berlin\""), &JsonKeyAttributes::default(), &PrefixExpander)
            .await
            .expect("expands")
            .expect("present");
        let reparsed = parser::parse(&serialize(&expanded)).expect("reparses");
        assert_eq!(reparsed, expanded);
    }

    /// Clause 4.9 EXAMPLE 11: with
    /// `?q=parkingTickets[value]=="Overstay 60 minutes"&jsonKeys=parkingTickets`
    /// "the target value raw is JSON, and is not expanded to ngsi-ld:hasValue
    /// using the core @context", so the member the filter addresses stays the
    /// literal object key.
    #[tokio::test]
    async fn a_json_keys_attribute_keeps_its_compound_value_member_literal() {
        let expr = parser::parse("parkingTickets[value]==\"Overstay 60 minutes\"").expect("parses");
        let declared: JsonKeyAttributes = ["https://example.com/parkingTickets".parse().expect("valid attribute name")]
            .into_iter()
            .collect();
        let expanded = expand_q_expr(expr, &declared, &PrefixExpander).await.expect("expands");
        let path = path_of(&expanded);
        assert_eq!(path.attr.as_str(), "https://example.com/parkingTickets", "the attribute itself still expands");
        assert_eq!(
            path.members()
                .expect("a member path")
                .addressed
                .iter()
                .map(AttributeName::as_str)
                .collect::<Vec<_>>(),
            vec!["value"]
        );
    }

    /// The same filter without the declaration expands the member, which is
    /// what makes the declaration observable at all.
    #[tokio::test]
    async fn the_same_member_expands_when_no_json_keys_declaration_names_the_attribute() {
        let expr = parser::parse("parkingTickets[value]==\"Overstay 60 minutes\"").expect("parses");
        let declared: JsonKeyAttributes = ["https://example.com/other".parse().expect("valid attribute name")].into_iter().collect();
        let expanded = expand_q_expr(expr, &declared, &PrefixExpander).await.expect("expands");
        assert_eq!(
            path_of(&expanded)
                .members()
                .expect("a member path")
                .addressed
                .iter()
                .map(AttributeName::as_str)
                .collect::<Vec<_>>(),
            vec!["https://example.com/value"]
        );
    }

    /// A declared attribute's NGSI-LD sub-attributes are not object keys, so
    /// they keep expanding through the `@context`.
    #[tokio::test]
    async fn a_json_keys_attribute_still_expands_its_ngsi_ld_sub_attributes() {
        let expr = parser::parse("parkingTickets.observedAt<2024-01-01T00:00:00Z").expect("parses");
        let declared: JsonKeyAttributes = ["https://example.com/parkingTickets".parse().expect("valid attribute name")]
            .into_iter()
            .collect();
        let expanded = expand_q_expr(expr, &declared, &PrefixExpander).await.expect("expands");
        assert_eq!(
            path_of(&expanded).sub_attrs.iter().map(AttributeName::as_str).collect::<Vec<_>>(),
            vec!["https://example.com/observedAt"]
        );
    }

    /// Clause 4.9: a `csf` targets the Context Source Properties of a
    /// registration, whose values are the raw JSON the registrant wrote, so a
    /// trailing path addresses their literal keys while the property and its
    /// sub-properties still expand.
    #[tokio::test]
    async fn a_csf_expands_its_properties_and_keeps_its_trailing_members_verbatim() {
        let expr = parser::parse("coverage.since[area.region]==\"north\"").expect("parses");
        let expanded = expand_csf_expr(expr, &PrefixExpander).await.expect("expands");
        let path = path_of(&expanded);
        assert_eq!(path.attr.as_str(), "https://example.com/coverage");
        assert_eq!(
            path.sub_attrs.iter().map(AttributeName::as_str).collect::<Vec<_>>(),
            vec!["https://example.com/since"]
        );
        assert_eq!(
            path.members()
                .expect("a member path")
                .addressed
                .iter()
                .map(AttributeName::as_str)
                .collect::<Vec<_>>(),
            vec!["area", "region"]
        );
        assert_eq!(
            path.members()
                .expect("a member path")
                .written
                .iter()
                .map(AttributeName::as_str)
                .collect::<Vec<_>>(),
            vec!["area", "region"]
        );
        let reparsed = parser::parse(&serialize(&expanded)).expect("reparses");
        assert_eq!(reparsed, expanded, "the stored text keeps the members as written");
    }

    #[tokio::test]
    async fn a_pattern_predicate_keeps_its_pattern_while_its_path_expands() {
        let expanded = expand_q_expression(Some("name~=\"^A.*\""), &JsonKeyAttributes::default(), &PrefixExpander)
            .await
            .expect("expands")
            .expect("present");
        match expanded {
            Expr::Pred(Predicate::MatchPattern { path, pattern }) => {
                assert_eq!(path.terminal().attr.as_str(), "https://example.com/name");
                assert_eq!(pattern.as_str(), "^A.*");
            }
            other @ (Expr::And(_) | Expr::Or(_) | Expr::Pred(_)) => panic!("expected a pattern predicate, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn the_comparison_operator_survives_expansion() {
        let expanded = expand_q_expression(Some("speed>=50"), &JsonKeyAttributes::default(), &PrefixExpander)
            .await
            .expect("expands")
            .expect("present");
        match expanded {
            Expr::Pred(Predicate::Ordering { op, .. }) => assert_eq!(op, OrderingOp::GreaterEq),
            other @ (Expr::And(_) | Expr::Or(_) | Expr::Pred(_)) => panic!("expected a comparison, got {other:?}"),
        }
    }
}
