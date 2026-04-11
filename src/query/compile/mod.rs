use crate::{
    core::error::AndromedaError,
    jsonld::{ContextSpec, JsonLdEngine, term_resolver},
    query::q::{ast::Expr, parser},
};

pub mod geo;

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
    let expr = parser::parse(q_str)?;
    let expanded = term_resolver::expand_q_expr(engine, expr, context).await?;
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
