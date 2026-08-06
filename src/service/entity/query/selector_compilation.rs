//! Compiling a query's entity selectors (clause 5.2.11) against the context.
//!
//! Shared by the entity and temporal query paths so the two cannot compile a
//! selection differently: a POST `entities[]` body is a union of independent
//! selectors on both, and flattening it into a comma-joined `id`/`type` pair
//! would match a `(id, type)` pairing no single selector named.

use crate::core::error::AndromedaError;
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine, term_expansion::ContextualExpander};
use andromeda_ngsi_ld_query::{
    entity_selection::EntitySelector,
    identifier::parse_id_list,
    pattern::Pattern,
    type_selection::{expansion::expand_type_expression, parser as type_parser},
};
use andromeda_ngsi_ld_request::query::entity_selector::EntitySelectorParams;

/// Compile a query's entity selectors against `context`.
///
/// A GET request — or any whose selection arrived as protocol headers — carries
/// no structured `entity_selectors`, so its flat `id`/`type`/`idPattern`
/// describe one implicit selector. A POST `entities[]` body carries one selector
/// per array entry, each compiled independently so their union is not flattened
/// into a cross product.
pub async fn compile_entity_selectors(
    engine: &JsonLdEngine,
    context: &ContextSpec,
    entity_selectors: &[EntitySelectorParams],
    flat_ids: Vec<String>,
    flat_id_pattern: Option<&str>,
    flat_type: Option<&str>,
) -> Result<Vec<EntitySelector>, AndromedaError> {
    if entity_selectors.is_empty() {
        return Ok(vec![compile_one(engine, context, flat_ids, flat_id_pattern, flat_type).await?]);
    }
    let mut selectors = Vec::with_capacity(entity_selectors.len());
    for raw in entity_selectors {
        let ids = raw.id.as_deref().map(split_id_list).unwrap_or_default();
        selectors.push(compile_one(engine, context, ids, raw.id_pattern.as_deref(), raw.type_.as_deref()).await?);
    }
    Ok(selectors)
}

/// Compile one selector's identifier list, identifier pattern and type
/// expression against the active context.
async fn compile_one(
    engine: &JsonLdEngine,
    context: &ContextSpec,
    ids: Vec<String>,
    id_pattern: Option<&str>,
    type_raw: Option<&str>,
) -> Result<EntitySelector, AndromedaError> {
    let ids = parse_id_list(&ids)?;
    let id_pattern = id_pattern
        .map(Pattern::parse)
        .transpose()
        .map_err(|error| AndromedaError::entity_validation(error.to_string()))?;
    let type_expr = match type_raw {
        Some(raw_types) => {
            let raw = type_parser::parse(raw_types)?;
            Some(expand_type_expression(&raw, &ContextualExpander::new(engine, context)).await?)
        }
        None => None,
    };
    Ok(EntitySelector::builder().ids(ids).id_pattern(id_pattern).type_expr(type_expr).build())
}

/// Split a comma-separated `id` list into its trimmed, non-empty members.
fn split_id_list(raw: &str) -> Vec<String> {
    raw.split(',').map(str::trim).filter(|id| !id.is_empty()).map(str::to_string).collect()
}
