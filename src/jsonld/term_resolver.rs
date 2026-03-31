//! Centralized term expansion and compaction for NGSI-LD queries and subscriptions.
//!
//! Functions in this module replace duplicated implementations that previously
//! lived in `subscription.rs`, `csource_subscription.rs`, `temporal.rs`, and
//! `entity/query.rs`.

use crate::{
    error::AndromedaError,
    jsonld::{CompactOptions, ContextSpec, ContextStripping, JsonLdEngine},
    query::q,
};
use serde_json::Value;
use std::collections::HashSet;

// ---------------------------------------------------------------------------
// Generic async q-expression walker
// ---------------------------------------------------------------------------

/// Recursively walk a [`q::Expr`] tree, applying an async handler to each
/// [`q::Predicate`] leaf.
///
/// `And` / `Or` branches are traversed recursively; `Pred` leaves are
/// transformed by `handler`.
async fn walk_q_expr<F, Fut>(expr: q::Expr, handler: &F) -> Result<q::Expr, AndromedaError>
where
    F: Fn(q::Predicate) -> Fut + Sync,
    Fut: std::future::Future<Output = Result<q::Predicate, AndromedaError>> + Send,
{
    match expr {
        q::Expr::And(terms) => {
            let mut expanded = Vec::with_capacity(terms.len());
            for t in terms {
                expanded.push(walk_q_expr_boxed(t, handler).await?);
            }
            Ok(q::Expr::And(expanded))
        }
        q::Expr::Or(terms) => {
            let mut expanded = Vec::with_capacity(terms.len());
            for t in terms {
                expanded.push(walk_q_expr_boxed(t, handler).await?);
            }
            Ok(q::Expr::Or(expanded))
        }
        q::Expr::Pred(pred) => {
            let expanded = handler(pred).await?;
            Ok(q::Expr::Pred(expanded))
        }
    }
}

/// Box-pinned wrapper for recursive async calls.
fn walk_q_expr_boxed<'a, F, Fut>(
    expr: q::Expr,
    handler: &'a F,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<q::Expr, AndromedaError>> + Send + 'a>>
where
    F: Fn(q::Predicate) -> Fut + Sync,
    Fut: std::future::Future<Output = Result<q::Predicate, AndromedaError>> + Send + 'a,
{
    Box::pin(walk_q_expr(expr, handler))
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Recursively expand attribute terms in a `q` expression.
///
/// Every attribute path (main attribute + sub-attributes) in the expression
/// is expanded to its full IRI form using the provided context.
pub fn expand_q_expr<'a>(
    engine: &'a JsonLdEngine,
    expr: q::Expr,
    context: &'a ContextSpec,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<q::Expr, AndromedaError>> + Send + 'a>> {
    Box::pin(async move { walk_q_expr(expr, &|pred| expand_pred_terms(engine, pred, context)).await })
}

/// Expand attribute terms in a single predicate.
///
/// The main attribute and all sub-attributes in the predicate's path are
/// expanded to full IRIs.
pub async fn expand_pred_terms(engine: &JsonLdEngine, pred: q::Predicate, context: &ContextSpec) -> Result<q::Predicate, AndromedaError> {
    match pred {
        q::Predicate::Exists(path) => {
            let path = expand_attr_path(engine, path, context).await?;
            Ok(q::Predicate::Exists(path))
        }
        q::Predicate::Compare { path, op, rhs } => {
            let path = expand_attr_path(engine, path, context).await?;
            Ok(q::Predicate::Compare { path, op, rhs })
        }
        q::Predicate::MatchPattern { path, pattern } => {
            let path = expand_attr_path(engine, path, context).await?;
            Ok(q::Predicate::MatchPattern { path, pattern })
        }
        q::Predicate::NotMatchPattern { path, pattern } => {
            let path = expand_attr_path(engine, path, context).await?;
            Ok(q::Predicate::NotMatchPattern { path, pattern })
        }
    }
}

/// Apply expandValues to an already-expanded q expression: for attributes in
/// `expand_attrs`, expand string scalar RHS values via the @context (spec 6.4.3.2).
pub fn apply_expand_values<'a>(
    engine: &'a JsonLdEngine,
    expr: q::Expr,
    expand_attrs: &'a HashSet<String>,
    context: &'a ContextSpec,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<q::Expr, AndromedaError>> + Send + 'a>> {
    Box::pin(async move { walk_q_expr(expr, &|pred| expand_compare_values(engine, pred, expand_attrs, context)).await })
}

async fn expand_compare_values(
    engine: &JsonLdEngine,
    pred: q::Predicate,
    expand_attrs: &HashSet<String>,
    context: &ContextSpec,
) -> Result<q::Predicate, AndromedaError> {
    match pred {
        q::Predicate::Compare { path, op, rhs } if expand_attrs.contains(&path.attr) => {
            let rhs = expand_rhs_values(engine, rhs, context).await?;
            Ok(q::Predicate::Compare { path, op, rhs })
        }
        q::Predicate::Exists(_) | q::Predicate::Compare { .. } | q::Predicate::MatchPattern { .. } | q::Predicate::NotMatchPattern { .. } => Ok(pred),
    }
}

async fn expand_rhs_values(engine: &JsonLdEngine, rhs: q::CompareValue, context: &ContextSpec) -> Result<q::CompareValue, AndromedaError> {
    match rhs {
        q::CompareValue::Scalar(q::Scalar::Str(s)) => {
            let expanded = engine.expand_term(&s, context).await?;
            Ok(q::CompareValue::Scalar(q::Scalar::Str(expanded)))
        }
        q::CompareValue::ValueList(values) => {
            let mut expanded = Vec::new();
            for v in values {
                if let q::Scalar::Str(s) = v {
                    let e = engine.expand_term(&s, context).await?;
                    expanded.push(q::Scalar::Str(e));
                } else {
                    expanded.push(v);
                }
            }
            Ok(q::CompareValue::ValueList(expanded))
        }
        q::CompareValue::Scalar(q::Scalar::Number(_) | q::Scalar::Bool(_) | q::Scalar::DateTime(_) | q::Scalar::Date(_) | q::Scalar::Time(_)) => Ok(rhs),
        q::CompareValue::Range { .. } => Ok(rhs),
    }
}

/// Apply jsonKeys to an already-expanded q expression: for attributes in
/// `json_keys_attrs`, expand sub-path keys (sub_attrs) via the @context (spec 6.4.3.2).
pub fn apply_json_keys<'a>(
    engine: &'a JsonLdEngine,
    expr: q::Expr,
    json_keys_attrs: &'a HashSet<String>,
    context: &'a ContextSpec,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<q::Expr, AndromedaError>> + Send + 'a>> {
    Box::pin(async move { walk_q_expr(expr, &|pred| expand_json_keys_sub_attrs(engine, pred, json_keys_attrs, context)).await })
}

async fn expand_json_keys_sub_attrs(
    engine: &JsonLdEngine,
    pred: q::Predicate,
    json_keys_attrs: &HashSet<String>,
    context: &ContextSpec,
) -> Result<q::Predicate, AndromedaError> {
    match pred {
        q::Predicate::Exists(path) if json_keys_attrs.contains(&path.attr) => {
            let path = expand_sub_attrs(engine, path, context).await?;
            Ok(q::Predicate::Exists(path))
        }
        q::Predicate::Compare { path, op, rhs } if json_keys_attrs.contains(&path.attr) => {
            let path = expand_sub_attrs(engine, path, context).await?;
            Ok(q::Predicate::Compare { path, op, rhs })
        }
        q::Predicate::MatchPattern { path, pattern } if json_keys_attrs.contains(&path.attr) => {
            let path = expand_sub_attrs(engine, path, context).await?;
            Ok(q::Predicate::MatchPattern { path, pattern })
        }
        q::Predicate::NotMatchPattern { path, pattern } if json_keys_attrs.contains(&path.attr) => {
            let path = expand_sub_attrs(engine, path, context).await?;
            Ok(q::Predicate::NotMatchPattern { path, pattern })
        }
        q::Predicate::Exists(_) | q::Predicate::Compare { .. } | q::Predicate::MatchPattern { .. } | q::Predicate::NotMatchPattern { .. } => Ok(pred),
    }
}

async fn expand_sub_attrs(engine: &JsonLdEngine, mut path: q::AttrPath, context: &ContextSpec) -> Result<q::AttrPath, AndromedaError> {
    let mut expanded = Vec::new();
    for sub in &path.sub_attrs {
        expanded.push(engine.expand_term(sub, context).await?);
    }
    path.sub_attrs = expanded;
    Ok(path)
}

/// Expand an attribute path (main attr + sub-attrs) to full IRIs.
pub async fn expand_attr_path(engine: &JsonLdEngine, mut path: q::AttrPath, context: &ContextSpec) -> Result<q::AttrPath, AndromedaError> {
    path.attr = engine.expand_term(&path.attr, context).await?;
    let mut expanded_sub = Vec::new();
    for sub in &path.sub_attrs {
        expanded_sub.push(engine.expand_term(sub, context).await?);
    }
    path.sub_attrs = expanded_sub;
    Ok(path)
}

/// Compact a type IRI back to a short term using the given context.
///
/// Returns the original IRI as-is if compaction fails or the IRI is not
/// an HTTP/URN type.
pub async fn compact_type_iri(engine: &JsonLdEngine, iri: &str, context: &ContextSpec) -> String {
    if !iri.starts_with("http") && !iri.starts_with("urn:") {
        return iri.to_string();
    }
    let doc = serde_json::json!({
        "@id": "urn:ngsi-ld:_:dummy",
        "@type": [iri]
    });
    match engine
        .compact(
            doc,
            context,
            CompactOptions {
                strip_context: ContextStripping::Strip,
            },
        )
        .await
    {
        Ok(compacted) => compacted
            .get("type")
            .and_then(|t| t.as_str())
            .map(String::from)
            .unwrap_or_else(|| iri.to_string()),
        Err(e) => {
            tracing::debug!(error = %e, iri = iri, "compact_type_iri fallback to raw IRI");
            iri.to_string()
        }
    }
}

/// Compact an attribute IRI back to a short term using the given context.
///
/// Returns the original IRI as-is if compaction fails or the IRI is not
/// an HTTP/URN type.
pub async fn compact_attr_iri(engine: &JsonLdEngine, iri: &str, context: &ContextSpec) -> String {
    if !iri.starts_with("http") && !iri.starts_with("urn:") {
        return iri.to_string();
    }
    let doc = serde_json::json!([{
        "@id": "urn:ngsi-ld:_:dummy",
        iri: [{"@value": "dummy"}]
    }]);
    match engine
        .compact(
            doc,
            context,
            CompactOptions {
                strip_context: ContextStripping::Strip,
            },
        )
        .await
    {
        Ok(compacted) => {
            if let Some(obj) = compacted.as_object() {
                for key in obj.keys() {
                    if key != "id" && key != "type" && !key.starts_with('@') {
                        return key.clone();
                    }
                }
            }
            iri.to_string()
        }
        Err(e) => {
            tracing::debug!(error = %e, iri = iri, "compact_attr_iri fallback to raw IRI");
            iri.to_string()
        }
    }
}

/// Validate that a jsonldContext value is a reachable, valid JSON-LD context.
///
/// Attempts to expand a minimal document using the given context. If
/// expansion succeeds the context is considered valid and reachable.
///
/// # Errors
///
/// Returns an error if the context is not a valid URI / JSON-LD context,
/// or if expansion fails (unreachable remote context, parse error, etc.).
pub async fn validate_jsonld_context_value(engine: &JsonLdEngine, value: &Value) -> Result<(), AndromedaError> {
    let ctx_value = match value {
        Value::String(s) => {
            let trimmed = s.trim();
            if trimmed.starts_with('{') || trimmed.starts_with('[') {
                serde_json::from_str::<Value>(trimmed).map_err(|e| AndromedaError::bad_request_data(format!("Invalid jsonldContext JSON: {}", e)))?
            } else {
                if !is_valid_uri(trimmed) {
                    return Err(AndromedaError::bad_request_data("jsonldContext must be a valid URI or JSON-LD context"));
                }
                Value::String(trimmed.to_string())
            }
        }
        Value::Array(_) | Value::Object(_) => value.clone(),
        Value::Null | Value::Bool(_) | Value::Number(_) => {
            return Err(AndromedaError::bad_request_data("jsonldContext must be a valid URI or JSON-LD context"));
        }
    };

    let doc = serde_json::json!({
        "@context": ctx_value,
        "id": "urn:ngsi-ld:_:context-validate",
        "type": "Subscription"
    });

    engine.expand(doc, &ContextSpec::none()).await.map_err(AndromedaError::from)?;
    Ok(())
}

/// Check if a string is a valid URI (URN or URL).
pub fn is_valid_uri(id: &str) -> bool {
    if id.starts_with("urn:") {
        return id.parse::<urn::Urn>().is_ok();
    }
    url::Url::parse(id).is_ok()
}
