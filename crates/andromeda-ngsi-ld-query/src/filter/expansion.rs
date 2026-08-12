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

use crate::{
    error::QueryError,
    filter::{
        expression::Expr,
        parser,
        path::{AttrPath, QueryPath, expanded_attribute_name},
        predicate::Predicate,
        scalar::{CompareValue, Scalar},
    },
    term_expander::TermExpander,
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use std::{collections::HashSet, future::Future, pin::Pin};

/// Parse and expand a `q` filter expression.
///
/// Returns `None` when `q` is absent or empty.
pub async fn expand_q_expression(q: Option<&str>, expander: &impl TermExpander) -> Result<Option<Expr>, QueryError> {
    let Some(q_str) = q.map(str::trim).filter(|trimmed| !trimmed.is_empty()) else {
        return Ok(None);
    };
    let expr = parser::parse(q_str)?;
    Ok(Some(expand_q_expr(expr, expander).await?))
}

/// Recursively expand attribute terms in a `q` expression.
///
/// Every attribute path (main attribute plus sub-attributes) in the expression
/// is expanded to its full IRI form.
pub async fn expand_q_expr(expr: Expr, expander: &impl TermExpander) -> Result<Expr, QueryError> {
    walk_predicates(expr, &|pred| expand_pred_terms(pred, expander)).await
}

/// Apply `expandValues`: for attributes in `expand_attrs`, expand string scalar
/// right-hand-side values through the `@context` (clause 6.4.3.2).
pub async fn apply_expand_values(expr: Expr, expand_attrs: &HashSet<AttributeName>, expander: &impl TermExpander) -> Result<Expr, QueryError> {
    walk_predicates(expr, &|pred| expand_compare_values(pred, expand_attrs, expander)).await
}

/// Apply `jsonKeys`: for attributes in `json_keys_attrs`, expand sub-path keys
/// through the `@context` (clause 6.4.3.2).
pub async fn apply_json_keys(expr: Expr, json_keys_attrs: &HashSet<AttributeName>, expander: &impl TermExpander) -> Result<Expr, QueryError> {
    walk_predicates(expr, &|pred| expand_json_keys_sub_attrs(pred, json_keys_attrs, expander)).await
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
async fn expand_pred_terms(pred: Predicate, expander: &impl TermExpander) -> Result<Predicate, QueryError> {
    match pred {
        Predicate::Exists(path) => Ok(Predicate::Exists(expand_query_path(path, expander).await?)),
        Predicate::Equality { path, membership, rhs } => Ok(Predicate::Equality {
            path: expand_query_path(path, expander).await?,
            membership,
            rhs,
        }),
        Predicate::Ordering { path, op, rhs } => Ok(Predicate::Ordering {
            path: expand_query_path(path, expander).await?,
            op,
            rhs,
        }),
        Predicate::MatchPattern { path, pattern } => Ok(Predicate::MatchPattern {
            path: expand_query_path(path, expander).await?,
            pattern,
        }),
        Predicate::NotMatchPattern { path, pattern } => Ok(Predicate::NotMatchPattern {
            path: expand_query_path(path, expander).await?,
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
pub async fn expand_query_path(path: QueryPath, expander: &impl TermExpander) -> Result<QueryPath, QueryError> {
    match path {
        QueryPath::Local(local) => Ok(QueryPath::Local(expand_attr_path(local, expander).await?)),
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
            let inner = expand_query_path_boxed(*inner, expander).await?;
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
fn expand_query_path_boxed<'a, E: TermExpander>(path: QueryPath, expander: &'a E) -> Pin<Box<dyn Future<Output = Result<QueryPath, QueryError>> + Send + 'a>>
where
    QueryPath: 'a,
{
    Box::pin(expand_query_path(path, expander))
}

/// Expand an attribute path — main attribute, sub-attributes and compound-value
/// member names — to full IRIs.
///
/// The member names of a compound value expand for the same reason the attribute
/// name does: JSON-LD expansion of a structured Property value turns its members
/// into IRI-keyed nodes, so `address[city]` addresses the expansion of `city`
/// under the request `@context`, not the four letters the client typed. Leaving
/// them compact addressed a member the stored payload does not carry.
pub async fn expand_attr_path(mut path: AttrPath, expander: &impl TermExpander) -> Result<AttrPath, QueryError> {
    path.attr = expanded_attribute_name(&expander.expand_term(path.attr.as_str()).await?)?;
    path.sub_attrs = expand_all(&path.sub_attrs, expander).await?;
    path.value_path = expand_all(&path.value_path, expander).await?;
    Ok(path)
}

/// Expand only the sub-attributes of an axis's terminal path, leaving every
/// attribute name alone.
///
/// `jsonKeys` names an already-expanded attribute whose *keys* are terms.
async fn expand_sub_attrs(mut path: QueryPath, expander: &impl TermExpander) -> Result<QueryPath, QueryError> {
    let expanded = expand_all(&path.terminal().sub_attrs, expander).await?;
    path.terminal_mut().sub_attrs = expanded;
    Ok(path)
}

/// Expand every term in a list.
async fn expand_all(terms: &[AttributeName], expander: &impl TermExpander) -> Result<Vec<AttributeName>, QueryError> {
    let mut expanded = Vec::with_capacity(terms.len());
    for term in terms {
        expanded.push(expanded_attribute_name(&expander.expand_term(term.as_str()).await?)?);
    }
    Ok(expanded)
}

/// Expand the right-hand side of a comparison when its attribute opted in via
/// `expandValues`.
async fn expand_compare_values(pred: Predicate, expand_attrs: &HashSet<AttributeName>, expander: &impl TermExpander) -> Result<Predicate, QueryError> {
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

/// Expand a single scalar operand: a string is a term to expand, every other
/// scalar kind is already a value.
async fn expand_scalar(scalar: Scalar, expander: &impl TermExpander) -> Result<Scalar, QueryError> {
    match scalar {
        Scalar::Str(s) => Ok(Scalar::Str(expander.expand_term(&s).await?.to_string())),
        other @ (Scalar::Number(_) | Scalar::Bool(_) | Scalar::DateTime(_) | Scalar::Date(_) | Scalar::Time(_)) => Ok(other),
    }
}

/// Expand the string operands of an equality comparison; every other scalar
/// kind is already a value, not a term.
async fn expand_rhs_values(rhs: CompareValue, expander: &impl TermExpander) -> Result<CompareValue, QueryError> {
    match rhs {
        CompareValue::Scalar(scalar) => Ok(CompareValue::Scalar(expand_scalar(scalar, expander).await?)),
        CompareValue::ValueList(values) => {
            let mut expanded = Vec::with_capacity(values.len());
            for value in values {
                expanded.push(expand_scalar(value, expander).await?);
            }
            Ok(CompareValue::ValueList(expanded))
        }
        // A range is bounded by two ordered values; neither can be a term.
        CompareValue::Range { .. } => Ok(rhs),
    }
}

/// Expand the sub-path keys of a predicate whose attribute opted in via
/// `jsonKeys`.
async fn expand_json_keys_sub_attrs(pred: Predicate, json_keys_attrs: &HashSet<AttributeName>, expander: &impl TermExpander) -> Result<Predicate, QueryError> {
    match pred {
        Predicate::Exists(path) if json_keys_attrs.contains(path.terminal().attr.as_str()) => Ok(Predicate::Exists(expand_sub_attrs(path, expander).await?)),
        Predicate::Equality { path, membership, rhs } if json_keys_attrs.contains(path.terminal().attr.as_str()) => Ok(Predicate::Equality {
            path: expand_sub_attrs(path, expander).await?,
            membership,
            rhs,
        }),
        Predicate::Ordering { path, op, rhs } if json_keys_attrs.contains(path.terminal().attr.as_str()) => Ok(Predicate::Ordering {
            path: expand_sub_attrs(path, expander).await?,
            op,
            rhs,
        }),
        Predicate::MatchPattern { path, pattern } if json_keys_attrs.contains(path.terminal().attr.as_str()) => Ok(Predicate::MatchPattern {
            path: expand_sub_attrs(path, expander).await?,
            pattern,
        }),
        Predicate::NotMatchPattern { path, pattern } if json_keys_attrs.contains(path.terminal().attr.as_str()) => Ok(Predicate::NotMatchPattern {
            path: expand_sub_attrs(path, expander).await?,
            pattern,
        }),
        Predicate::Exists(_) | Predicate::Equality { .. } | Predicate::Ordering { .. } | Predicate::MatchPattern { .. } | Predicate::NotMatchPattern { .. } => {
            Ok(pred)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::{predicate::OrderingOp, serialize::serialize};
    use iri_rs::IriBuf;

    /// An expander that prefixes every term, so a test can see exactly which
    /// positions were rewritten without standing up a JSON-LD engine.
    struct PrefixExpander;

    impl TermExpander for PrefixExpander {
        async fn expand_term(&self, term: &str) -> Result<IriBuf, QueryError> {
            Ok(IriBuf::new(format!("https://example.com/{term}")).expect("valid IRI"))
        }

        async fn expand_type(&self, term: &str) -> Result<IriBuf, QueryError> {
            Ok(IriBuf::new(format!("https://example.com/types/{term}")).expect("valid IRI"))
        }
    }

    fn opted_in(names: &[&str]) -> HashSet<AttributeName> {
        names.iter().map(|name| name.parse().expect("valid attribute name")).collect()
    }

    fn path_of(expr: &Expr) -> &AttrPath {
        match expr {
            Expr::Pred(Predicate::Equality { path, .. }) | Expr::Pred(Predicate::Ordering { path, .. }) => path.terminal(),
            other => unreachable!("expected a comparison, got {other:?}"),
        }
    }

    fn equality_rhs(expr: &Expr) -> &CompareValue {
        match expr {
            Expr::Pred(Predicate::Equality { rhs, .. }) => rhs,
            other => unreachable!("expected an equality comparison, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn an_empty_filter_expands_to_nothing() {
        assert!(expand_q_expression(None, &PrefixExpander).await.expect("no filter").is_none());
        assert!(expand_q_expression(Some("   "), &PrefixExpander).await.expect("blank filter").is_none());
    }

    #[tokio::test]
    async fn every_path_segment_of_every_predicate_is_expanded() {
        let expanded = expand_q_expression(Some("speed>50;temperature.observedAt<2024-01-01T00:00:00Z"), &PrefixExpander)
            .await
            .expect("expands")
            .expect("present");
        let Expr::And(terms) = expanded else {
            unreachable!("expected a conjunction");
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
        let expanded = expand_q_expression(Some("isParkedIn{Building,Store:category==\"x\"}"), &PrefixExpander)
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
                QueryPath::Local(local) => unreachable!("expected a hop, got {local:?}"),
            },
            other => unreachable!("expected an equality predicate, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn nested_hops_are_expanded_at_every_level() {
        let expanded = expand_q_expression(Some("outer{inner{zone==\"north\"}}"), &PrefixExpander)
            .await
            .expect("expands")
            .expect("present");
        let rendered = parser::parse(&serialize(&expanded)).expect("reparses");
        assert_eq!(rendered, expanded, "an expanded traversal must round-trip");
        assert_eq!(path_of(&expanded).attr.as_str(), "https://example.com/zone");
    }

    #[tokio::test]
    async fn expand_values_rewrites_only_the_named_attributes() {
        let expr = parser::parse("colour==\"red\";label==\"red\"").expect("parses");
        let opted_in = opted_in(&["colour"]);
        let expanded = apply_expand_values(expr, &opted_in, &PrefixExpander).await.expect("expands");
        let Expr::And(terms) = expanded else {
            unreachable!("expected a conjunction");
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

    /// JSON-LD expansion of a structured Property value turns its members into
    /// IRI-keyed nodes, so a compound-value member has to be expanded through the
    /// same `@context` the attribute name is. Leaving it compact addressed a
    /// member the stored payload does not carry.
    #[tokio::test]
    async fn a_compound_value_member_is_expanded_through_the_context() {
        let expanded = expand_q_expression(Some("address[city]==\"Berlin\""), &PrefixExpander)
            .await
            .expect("expands")
            .expect("present");
        let path = path_of(&expanded);
        assert_eq!(path.attr.as_str(), "https://example.com/address");
        assert_eq!(
            path.value_path.iter().map(AttributeName::as_str).collect::<Vec<_>>(),
            vec!["https://example.com/city"]
        );
    }

    #[tokio::test]
    async fn every_member_of_a_nested_compound_path_is_expanded() {
        let expanded = expand_q_expression(Some("sensor.rawdata[airquality.particulate]==40"), &PrefixExpander)
            .await
            .expect("expands")
            .expect("present");
        let path = path_of(&expanded);
        assert_eq!(
            path.sub_attrs.iter().map(AttributeName::as_str).collect::<Vec<_>>(),
            vec!["https://example.com/rawdata"]
        );
        assert_eq!(
            path.value_path.iter().map(AttributeName::as_str).collect::<Vec<_>>(),
            vec!["https://example.com/airquality", "https://example.com/particulate"]
        );
    }

    #[tokio::test]
    async fn an_expanded_compound_path_round_trips_through_the_serialisation() {
        let expanded = expand_q_expression(Some("address[city]==\"Berlin\""), &PrefixExpander)
            .await
            .expect("expands")
            .expect("present");
        let reparsed = parser::parse(&serialize(&expanded)).expect("reparses");
        assert_eq!(reparsed, expanded);
    }

    #[tokio::test]
    async fn json_keys_rewrites_sub_paths_without_touching_the_attribute() {
        let expr = parser::parse("address.city==\"Berlin\"").expect("parses");
        let opted_in = opted_in(&["address"]);
        let expanded = apply_json_keys(expr, &opted_in, &PrefixExpander).await.expect("expands");
        let path = path_of(&expanded);
        assert_eq!(path.attr.as_str(), "address", "jsonKeys names an attribute, it does not expand it");
        assert_eq!(
            path.sub_attrs.iter().map(AttributeName::as_str).collect::<Vec<_>>(),
            vec!["https://example.com/city"]
        );
    }

    #[tokio::test]
    async fn json_keys_leaves_predicates_on_other_attributes_alone() {
        let expr = parser::parse("address.city==\"Berlin\"").expect("parses");
        let opted_in = opted_in(&["somethingElse"]);
        let expanded = apply_json_keys(expr, &opted_in, &PrefixExpander).await.expect("expands");
        let path = path_of(&expanded);
        assert_eq!(path.sub_attrs.iter().map(AttributeName::as_str).collect::<Vec<_>>(), vec!["city"]);
    }

    #[tokio::test]
    async fn a_pattern_predicate_keeps_its_pattern_while_its_path_expands() {
        let expanded = expand_q_expression(Some("name~=\"^A.*\""), &PrefixExpander)
            .await
            .expect("expands")
            .expect("present");
        match expanded {
            Expr::Pred(Predicate::MatchPattern { path, pattern }) => {
                assert_eq!(path.terminal().attr.as_str(), "https://example.com/name");
                assert_eq!(pattern.as_str(), "^A.*");
            }
            other => unreachable!("expected a pattern predicate, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn the_comparison_operator_survives_expansion() {
        let expanded = expand_q_expression(Some("speed>=50"), &PrefixExpander)
            .await
            .expect("expands")
            .expect("present");
        match expanded {
            Expr::Pred(Predicate::Ordering { op, .. }) => assert_eq!(op, OrderingOp::GreaterEq),
            other => unreachable!("expected a comparison, got {other:?}"),
        }
    }
}
