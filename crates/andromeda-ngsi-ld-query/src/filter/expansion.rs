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
        path::{AttrPath, expanded_attribute_name},
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
        Predicate::Exists(path) => Ok(Predicate::Exists(expand_attr_path(path, expander).await?)),
        Predicate::Compare { path, op, rhs } => Ok(Predicate::Compare {
            path: expand_attr_path(path, expander).await?,
            op,
            rhs,
        }),
        Predicate::MatchPattern { path, pattern } => Ok(Predicate::MatchPattern {
            path: expand_attr_path(path, expander).await?,
            pattern,
        }),
        Predicate::NotMatchPattern { path, pattern } => Ok(Predicate::NotMatchPattern {
            path: expand_attr_path(path, expander).await?,
            pattern,
        }),
    }
}

/// Expand an attribute path — main attribute plus sub-attributes — to full IRIs.
pub async fn expand_attr_path(mut path: AttrPath, expander: &impl TermExpander) -> Result<AttrPath, QueryError> {
    path.attr = expanded_attribute_name(&expander.expand_term(path.attr.as_str()).await?)?;
    path.sub_attrs = expand_all(&path.sub_attrs, expander).await?;
    Ok(path)
}

/// Expand only the sub-attributes of a path, leaving the main attribute alone.
///
/// `jsonKeys` names an already-expanded attribute whose *keys* are terms.
async fn expand_sub_attrs(mut path: AttrPath, expander: &impl TermExpander) -> Result<AttrPath, QueryError> {
    path.sub_attrs = expand_all(&path.sub_attrs, expander).await?;
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
        Predicate::Compare { path, op, rhs } if expand_attrs.contains(path.attr.as_str()) => Ok(Predicate::Compare {
            path,
            op,
            rhs: expand_rhs_values(rhs, expander).await?,
        }),
        Predicate::Exists(_) | Predicate::Compare { .. } | Predicate::MatchPattern { .. } | Predicate::NotMatchPattern { .. } => Ok(pred),
    }
}

/// Expand the string operands of a comparison; every other scalar kind is
/// already a value, not a term.
async fn expand_rhs_values(rhs: CompareValue, expander: &impl TermExpander) -> Result<CompareValue, QueryError> {
    match rhs {
        CompareValue::Scalar(Scalar::Str(s)) => Ok(CompareValue::Scalar(Scalar::Str(expander.expand_term(&s).await?.to_string()))),
        CompareValue::ValueList(values) => {
            let mut expanded = Vec::with_capacity(values.len());
            for value in values {
                match value {
                    Scalar::Str(s) => expanded.push(Scalar::Str(expander.expand_term(&s).await?.to_string())),
                    other @ (Scalar::Number(_) | Scalar::Bool(_) | Scalar::DateTime(_) | Scalar::Date(_) | Scalar::Time(_)) => expanded.push(other),
                }
            }
            Ok(CompareValue::ValueList(expanded))
        }
        // A range is bounded by two ordered values; neither can be a term.
        CompareValue::Scalar(Scalar::Number(_) | Scalar::Bool(_) | Scalar::DateTime(_) | Scalar::Date(_) | Scalar::Time(_)) | CompareValue::Range { .. } => {
            Ok(rhs)
        }
    }
}

/// Expand the sub-path keys of a predicate whose attribute opted in via
/// `jsonKeys`.
async fn expand_json_keys_sub_attrs(pred: Predicate, json_keys_attrs: &HashSet<AttributeName>, expander: &impl TermExpander) -> Result<Predicate, QueryError> {
    match pred {
        Predicate::Exists(path) if json_keys_attrs.contains(path.attr.as_str()) => Ok(Predicate::Exists(expand_sub_attrs(path, expander).await?)),
        Predicate::Compare { path, op, rhs } if json_keys_attrs.contains(path.attr.as_str()) => Ok(Predicate::Compare {
            path: expand_sub_attrs(path, expander).await?,
            op,
            rhs,
        }),
        Predicate::MatchPattern { path, pattern } if json_keys_attrs.contains(path.attr.as_str()) => Ok(Predicate::MatchPattern {
            path: expand_sub_attrs(path, expander).await?,
            pattern,
        }),
        Predicate::NotMatchPattern { path, pattern } if json_keys_attrs.contains(path.attr.as_str()) => Ok(Predicate::NotMatchPattern {
            path: expand_sub_attrs(path, expander).await?,
            pattern,
        }),
        Predicate::Exists(_) | Predicate::Compare { .. } | Predicate::MatchPattern { .. } | Predicate::NotMatchPattern { .. } => Ok(pred),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::predicate::CmpOp;
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

    fn compare(expr: &Expr) -> (&AttrPath, &CompareValue) {
        match expr {
            Expr::Pred(Predicate::Compare { path, rhs, .. }) => (path, rhs),
            other => unreachable!("expected a comparison, got {other:?}"),
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
        let (first, _) = compare(&terms[0]);
        assert_eq!(first.attr.as_str(), "https://example.com/speed");
        let (second, _) = compare(&terms[1]);
        assert_eq!(second.attr.as_str(), "https://example.com/temperature");
        assert_eq!(
            second.sub_attrs.iter().map(AttributeName::as_str).collect::<Vec<_>>(),
            vec!["https://example.com/observedAt"]
        );
    }

    #[tokio::test]
    async fn expand_values_rewrites_only_the_named_attributes() {
        let expr = parser::parse("colour==\"red\";label==\"red\"").expect("parses");
        let opted_in = opted_in(&["colour"]);
        let expanded = apply_expand_values(expr, &opted_in, &PrefixExpander).await.expect("expands");
        let Expr::And(terms) = expanded else {
            unreachable!("expected a conjunction");
        };
        assert_eq!(compare(&terms[0]).1, &CompareValue::Scalar(Scalar::Str("https://example.com/red".to_string())));
        assert_eq!(compare(&terms[1]).1, &CompareValue::Scalar(Scalar::Str("red".to_string())));
    }

    #[tokio::test]
    async fn expand_values_leaves_non_string_operands_alone() {
        let expr = parser::parse("speed==10..20").expect("parses");
        let opted_in = opted_in(&["speed"]);
        let expanded = apply_expand_values(expr, &opted_in, &PrefixExpander).await.expect("expands");
        assert_eq!(
            compare(&expanded).1,
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
            compare(&expanded).1,
            &CompareValue::ValueList(vec![
                Scalar::Str("https://example.com/red".to_string()),
                Scalar::Str("https://example.com/blue".to_string()),
            ])
        );
    }

    #[tokio::test]
    async fn json_keys_rewrites_sub_paths_without_touching_the_attribute() {
        let expr = parser::parse("address.city==\"Berlin\"").expect("parses");
        let opted_in = opted_in(&["address"]);
        let expanded = apply_json_keys(expr, &opted_in, &PrefixExpander).await.expect("expands");
        let (path, _) = compare(&expanded);
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
        let (path, _) = compare(&expanded);
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
                assert_eq!(path.attr.as_str(), "https://example.com/name");
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
            Expr::Pred(Predicate::Compare { op, .. }) => assert_eq!(op, CmpOp::GreaterEq),
            other => unreachable!("expected a comparison, got {other:?}"),
        }
    }
}
