//! Compiling a query's entity selectors (clause 5.2.11) against the context.
//!
//! Shared by the entity and temporal query paths so the two cannot compile a
//! selection differently: a POST `entities[]` body is a union of independent
//! selectors on both, and flattening it into a comma-joined `id`/`type` pair
//! would match a `(id, type)` pairing no single selector named.

use andromeda_jsonld::{
    engine::{processor::JsonLdEngine, term_lookup::PreparedTermContext},
    term_expansion::ContextualExpander,
};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_query::{
    entity_selection::EntitySelector,
    error::QueryError,
    pattern::Pattern,
    type_selection::{expansion::expand_type_expression, grammar::TypeExprRaw},
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
    context: &PreparedTermContext,
    entity_selectors: &[EntitySelectorParams],
    flat_ids: Vec<Uri>,
    flat_id_pattern: Option<&Pattern>,
    flat_type: Option<&TypeExprRaw>,
) -> Result<Vec<EntitySelector>, QueryError> {
    if entity_selectors.is_empty() {
        return Ok(vec![compile_one(engine, context, flat_ids, flat_id_pattern, flat_type).await?]);
    }
    let mut selectors = Vec::with_capacity(entity_selectors.len());
    for raw in entity_selectors {
        let ids = raw.id.clone().unwrap_or_default();
        selectors.push(compile_one(engine, context, ids, raw.id_pattern.as_ref(), raw.type_.as_ref()).await?);
    }
    Ok(selectors)
}

/// Compile one selector's identifier list, identifier pattern and type
/// expression against the active context.
async fn compile_one(
    engine: &JsonLdEngine,
    context: &PreparedTermContext,
    ids: Vec<Uri>,
    id_pattern: Option<&Pattern>,
    type_raw: Option<&TypeExprRaw>,
) -> Result<EntitySelector, QueryError> {
    let id_pattern = id_pattern.cloned();
    let type_expr = match type_raw {
        Some(raw) => Some(expand_type_expression(raw, &ContextualExpander::new(engine, context)).await?),
        None => None,
    };
    Ok(EntitySelector::builder().ids(ids).id_pattern(id_pattern).type_expr(type_expr).build())
}
