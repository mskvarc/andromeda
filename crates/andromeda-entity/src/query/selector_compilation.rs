//! Compiling a query's entity selectors (clause 5.2.33) against the context.
//!
//! Shared by the entity and temporal query paths so the two cannot compile a
//! selection differently: a POST `entities[]` body is a union of independent
//! selectors on both, and flattening it into a comma-joined `id`/`type` pair
//! would match a `(id, type)` pairing no single selector named.

use andromeda_jsonld::{
    engine::{processor::JsonLdEngine, term_lookup::PreparedTermContext},
    term_expansion::ContextualExpander,
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::{
    entity_selection::EntitySelector,
    error::QueryError,
    pattern::Pattern,
    type_selection::{expansion::expand_type_expression, grammar::TypeExprRaw},
};
use andromeda_ngsi_ld_request::query::entity_selector::EntitySelectorParams;

/// Compile a query's effective entity selectors against `context`.
///
/// `selectors` is the union the query asks for (see
/// `EntitySelectorParams::effective`): a POST `entities[]` body contributes one
/// per array entry, each compiled independently so their union is not flattened
/// into a cross product, and a GET request contributes its one implicit selector.
///
/// # Errors
///
/// Reports [`QueryError`] when a selector's type expression is not well formed
/// or one of its terms cannot be expanded against `context`.
pub async fn compile_entity_selectors(
    engine: &JsonLdEngine,
    context: &PreparedTermContext,
    selectors: &[EntitySelectorParams],
) -> Result<Vec<EntitySelector>, QueryError> {
    let mut compiled = Vec::with_capacity(selectors.len());
    for raw in selectors {
        let ids = raw.id.clone().unwrap_or_default();
        compiled.push(compile_one(engine, context, ids, raw.id_pattern.as_ref(), raw.type_.as_ref()).await?);
    }
    Ok(compiled)
}

/// Compile one selector's identifier list, identifier pattern and type
/// expression against the active context.
async fn compile_one(
    engine: &JsonLdEngine,
    context: &PreparedTermContext,
    ids: Vec<Identifier>,
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
