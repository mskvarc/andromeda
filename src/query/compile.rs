use crate::{
    error::AndromedaError,
    jsonld::{ContextSpec, JsonLdEngine},
    query::q::Expr,
};

/// Expand a list of short attribute names to their full IRIs.
pub async fn expand_attrs(raw: &[String], engine: &JsonLdEngine, context: &ContextSpec) -> Result<Vec<String>, AndromedaError> {
    let mut expanded = Vec::with_capacity(raw.len());
    for a in raw {
        expanded.push(engine.expand_term(a, context).await?);
    }
    Ok(expanded)
}

/// Parse and expand a `q` filter expression.
///
/// Returns `None` when `q` is absent or empty.
pub async fn expand_q_expression(q: Option<&str>, engine: &JsonLdEngine, context: &ContextSpec) -> Result<Option<Expr>, AndromedaError> {
    let q_str = match q {
        Some(s) if !s.trim().is_empty() => s.trim(),
        _ => return Ok(None),
    };
    let expr = crate::query::q::parse(q_str)?;
    let expanded = crate::jsonld::term_resolver::expand_q_expr(engine, expr, context).await?;
    Ok(Some(expanded))
}

/// Expand a pick/omit term list, keeping "id", "type", "scope" as-is.
pub async fn expand_projection_terms(raw: &[String], engine: &JsonLdEngine, context: &ContextSpec) -> Result<Vec<String>, AndromedaError> {
    let mut expanded = Vec::with_capacity(raw.len());
    for term in raw {
        if matches!(term.as_str(), "id" | "type" | "scope") {
            expanded.push(term.clone());
        } else {
            expanded.push(engine.expand_term(term, context).await?);
        }
    }
    Ok(expanded)
}

/// Validate an idPattern is a valid regex.
pub fn validate_id_pattern(pattern: &str) -> Result<(), AndromedaError> {
    regex::Regex::new(pattern).map_err(|_| AndromedaError::bad_request_data(format!("Invalid idPattern regex: {pattern}")))?;
    Ok(())
}

/// Expand a comma-separated list of entity type short names to IRIs.
pub async fn expand_entity_types(raw_types: &str, engine: &JsonLdEngine, context: &ContextSpec) -> Result<Vec<String>, AndromedaError> {
    let mut expanded = Vec::new();
    for t in raw_types.split(',').map(str::trim) {
        if !t.is_empty() {
            expanded.push(engine.expand_type(t, context).await?);
        }
    }
    Ok(expanded)
}
