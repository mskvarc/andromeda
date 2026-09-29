//! The entity store answering registration policy's admission questions
//! (clauses 5.9.2.4 and 5.9.3.4).
//!
//! Registration policy asks whether this broker already holds context data a
//! proxied registration claims; the entity crate owns the reads that answer it.
//! Neither crate may name the other for this, so the adapter lives here, beside
//! the other cross-service joins the runtime composes.

use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{
    error::EntityError,
    port::{query::EntityQueryStore, reader::EntityReader},
};
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine, term_expansion::ContextualExpander};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{entity_info::EntityInfo, expanded::node::expanded_object_ref};
use andromeda_ngsi_ld_query::{
    entity::compiled::CompiledEntityQuery,
    entity_selection::{EntitySelection, EntitySelector as CompiledEntitySelector},
    pagination::{CountPolicy, Pagination},
    type_selection::expansion::expand_type_expression,
};
use andromeda_registration::{
    entity_probe::{EntityPresence, RegisteredEntityProbe},
    error::RegistrationError,
};
use futures::future::BoxFuture;
use iri_rs::IriBuf;
use std::sync::Arc;

/// Answers registration admission questions from the tenant's entity store.
pub(crate) struct EntityStoreRegistrationProbe {
    /// Reads one entity's expanded payload.
    reader: Arc<dyn EntityReader>,
    /// Runs compiled identifier queries.
    query: Arc<dyn EntityQueryStore>,
    /// Compiles a registration's type expression, whose leaves are already
    /// expanded IRIs, into the query form.
    engine: JsonLdEngine,
    /// Tenant live branch registration admission inspects.
    live_target: ContextTarget,
}

impl EntityStoreRegistrationProbe {
    /// Create a probe over the tenant's entity reader and query store.
    pub(crate) const fn new(
        reader: Arc<dyn EntityReader>,
        query: Arc<dyn EntityQueryStore>,
        engine: JsonLdEngine,
        live_target: ContextTarget,
    ) -> EntityStoreRegistrationProbe {
        EntityStoreRegistrationProbe {
            reader,
            query,
            engine,
            live_target,
        }
    }

    /// Compile one registration selector into the query form.
    ///
    /// Registration `information` is expanded before policy sees it (clause
    /// 5.5.7), so the selector's type leaves are absolute IRIs; expanding them
    /// against the core context leaves them unchanged and yields the typed
    /// expression the store evaluates.
    async fn compile_selector(&self, selector: &EntityInfo) -> Result<CompiledEntitySelector, RegistrationError> {
        let prepared = self.engine.prepare_term_context(&ContextSpec::core_only());
        let type_expr = expand_type_expression(&selector.type_.as_type_expression(), &ContextualExpander::new(&self.engine, &prepared)).await?;
        Ok(CompiledEntitySelector::builder()
            .ids(selector.id.iter().cloned().collect()) // clone: the compiled selector owns its identifiers across the store await.
            .id_pattern(selector.id_pattern.clone()) // clone: same reason.
            .type_expr(Some(type_expr))
            .build())
    }
}

/// Erase an entity-store failure at the registration policy boundary.
fn probe_error(error: &EntityError) -> RegistrationError {
    RegistrationError::internal(format!("entity store could not answer a registration admission check: {error}"))
}

impl RegisteredEntityProbe for EntityStoreRegistrationProbe {
    fn holds_any_attribute<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        entity_id: &'life1 Identifier,
        attributes: &'life2 [IriBuf],
    ) -> BoxFuture<'async_trait, Result<EntityPresence, RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            let Some(entity) = self.reader.get(&self.live_target, entity_id).await.map_err(|error| probe_error(&error))? else {
                return Ok(EntityPresence::Absent);
            };
            // The reader hands back the stored expanded payload, which wraps
            // its node in an array; reading it as a bare object found no
            // member and let an exclusive registration claim an attribute
            // this broker already holds (clause 5.9.2.4).
            let holds = expanded_object_ref(&entity).is_some_and(|members| attributes.iter().any(|attribute| members.contains_key(attribute.as_str())));
            Ok(if holds { EntityPresence::Present } else { EntityPresence::Absent })
        })
    }

    fn any_entity_matching<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        selector: Option<&'life1 EntityInfo>,
        attributes: &'life2 [IriBuf],
    ) -> BoxFuture<'async_trait, Result<EntityPresence, RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            let selectors = match selector {
                Some(selector) => vec![self.compile_selector(selector).await?],
                None => Vec::new(),
            };
            let compiled = CompiledEntityQuery::builder()
                .selection(
                    EntitySelection::builder()
                        .selectors(selectors)
                        .attrs(attributes.to_vec()) // clone: the compiled selection owns the attribute IRIs across the store await.
                        .build(),
                )
                .pagination(Pagination {
                    limit: 1,
                    offset: 0,
                    count: CountPolicy::Omit,
                })
                .build();
            let outcome = self.query.query_ids(&self.live_target, &compiled).await.map_err(|error| probe_error(&error))?;
            // An identifier-only read leaves nothing to finish a declined
            // predicate against, so a store that declined one cannot answer.
            if !outcome.unapplied.is_empty() {
                return Err(RegistrationError::internal(
                    "entity store declined a predicate of a registration admission check",
                ));
            }
            Ok(if outcome.rows.is_empty() {
                EntityPresence::Absent
            } else {
                EntityPresence::Present
            })
        })
    }
}
