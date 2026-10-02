//! The entity store answering registration policy's admission questions
//! (clauses 5.9.2.4 and 5.9.3.4).
//!
//! Registration policy asks whether this broker already holds context data a
//! proxied registration claims; the entity crate owns the reads that answer it.
//! Neither crate may name the other for this, so the adapter lives here, beside
//! the other cross-service joins the runtime composes.
//!
//! Both questions are identifier-only store queries narrowed by `attrs`
//! (clause 5.7.2: an entity holding at least one of the attributes), so no
//! entity payload is read to answer them.

use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{error::EntityError, port::query::EntityQueryStore};
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine, term_expansion::ContextualExpander};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::entity_info::EntityInfo;
use andromeda_ngsi_ld_query::{
    entity::compiled::CompiledEntityQuery,
    entity_selection::{EntitySelection, EntitySelector as CompiledEntitySelector},
    pagination::{CountPolicy, Pagination},
    type_selection::expansion::expand_type_expression,
};
use andromeda_registration::{
    entity_probe::{EntityPresence, RegisteredEntityProbe},
    error::RegistrationError,
    registration_invariant::RegistrationInvariant,
};
use futures::future::BoxFuture;
use iri_rs::IriBuf;
use std::sync::Arc;

/// Answers registration admission questions from the tenant's entity store.
pub(crate) struct EntityStoreRegistrationProbe {
    /// Runs compiled identifier queries.
    query: Arc<dyn EntityQueryStore>,
    /// Compiles a registration's type expression, whose leaves are already
    /// expanded IRIs, into the query form.
    engine: JsonLdEngine,
    /// Tenant live branch registration admission inspects.
    live_target: ContextTarget,
}

impl EntityStoreRegistrationProbe {
    /// Create a probe over the tenant's entity query store.
    pub(crate) const fn new(query: Arc<dyn EntityQueryStore>, engine: JsonLdEngine, live_target: ContextTarget) -> EntityStoreRegistrationProbe {
        EntityStoreRegistrationProbe { query, engine, live_target }
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

    /// Whether the store holds an entity one of `selectors` admits (every
    /// entity when there is none) that has at least one of `attributes`
    /// (every entity when there is none).
    async fn presence(&self, selectors: Vec<CompiledEntitySelector>, attributes: &[IriBuf]) -> Result<EntityPresence, RegistrationError> {
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
            return Err(RegistrationInvariant::EntityProbeDeclinedPredicate.into());
        }
        Ok(if outcome.rows.is_empty() {
            EntityPresence::Absent
        } else {
            EntityPresence::Present
        })
    }
}

/// Classify an entity-store failure at the registration policy boundary:
/// backpressure and a store fault keep their categories, and any other entity
/// failure is recorded and reported as the admission check it broke.
fn probe_error(error: &EntityError) -> RegistrationError {
    match error {
        EntityError::StoreSaturated | EntityError::TransactionContended => RegistrationError::StoreSaturated,
        EntityError::StoreUnavailable => RegistrationError::StoreUnavailable,
        EntityError::Persistence(failure) => RegistrationError::from(*failure),
        other @ (EntityError::Validation(_)
        | EntityError::NotFound { .. }
        | EntityError::TypeNotFound { .. }
        | EntityError::AttributeNotFound { .. }
        | EntityError::MemberNotFound(_)
        | EntityError::AlreadyExists { .. }
        | EntityError::Contention { .. }
        | EntityError::OperationNotSupported(_)
        | EntityError::PayloadBudget(_)
        | EntityError::JsonLd(_)
        | EntityError::Query(_)
        | EntityError::Request(_)
        | EntityError::Representation(_)
        | EntityError::Commit(_)
        | EntityError::Internal(_)) => {
            tracing::error!(error = %other, "the entity store could not answer a registration admission check");
            RegistrationInvariant::EntityProbe.into()
        }
    }
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
            // No attribute named means none is held; an empty `attrs` would
            // instead select the entity whatever it holds.
            if attributes.is_empty() {
                return Ok(EntityPresence::Absent);
            }
            let selector = CompiledEntitySelector::builder()
                .ids(vec![entity_id.clone()]) // clone: the compiled selector owns its identifier across the store await.
                .id_pattern(None)
                .type_expr(None)
                .build();
            self.presence(vec![selector], attributes).await
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
            self.presence(selectors, attributes).await
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::registered_entity_probe::EntityStoreRegistrationProbe;
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_entity::{
        error::EntityError,
        pagination_outcome::PaginationOutcome,
        port::{query::EntityQueryStore, retrieved_entity::RetrievedEntity},
        query_outcome::QueryOutcome,
        query_residual::ResidualFilter,
        storage_profile::StorageProfile,
    };
    use andromeda_jsonld::engine::processor::JsonLdEngine;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_query::entity::compiled::CompiledEntityQuery;
    use andromeda_registration::entity_probe::{EntityPresence, RegisteredEntityProbe};
    use futures::future::BoxFuture;
    use iri_rs::IriBuf;
    use std::sync::{Arc, Mutex};

    const SPEED: &str = "https://example.org/speed";
    const BRAND: &str = "https://example.org/brand";

    /// A store holding one entity with the attributes it names, answering an
    /// identifier query as the PostgreSQL store does: the entities a selector
    /// admits that hold one of the `attrs`. It records every query it is
    /// asked, and refuses to read a payload.
    struct AttributeIndex {
        entity: Identifier,
        attributes: Vec<IriBuf>,
        asked: Mutex<Vec<CompiledEntityQuery>>,
    }

    impl EntityQueryStore for AttributeIndex {
        fn profile(&self) -> &StorageProfile {
            &StorageProfile::POSTGIS_LTREE
        }

        fn query_compiled<'life0, 'life1, 'life2, 'async_trait>(
            &'life0 self,
            _target: &'life1 ContextTarget,
            _compiled: &'life2 CompiledEntityQuery,
        ) -> BoxFuture<'async_trait, Result<QueryOutcome<RetrievedEntity>, EntityError>>
        where
            'life0: 'async_trait,
            'life1: 'async_trait,
            'life2: 'async_trait,
            Self: 'async_trait,
        {
            panic!("an admission check reads no entity payload")
        }

        fn query_ids<'life0, 'life1, 'life2, 'async_trait>(
            &'life0 self,
            _target: &'life1 ContextTarget,
            compiled: &'life2 CompiledEntityQuery,
        ) -> BoxFuture<'async_trait, Result<QueryOutcome<Identifier>, EntityError>>
        where
            'life0: 'async_trait,
            'life1: 'async_trait,
            'life2: 'async_trait,
            Self: 'async_trait,
        {
            self.asked.lock().expect("the record is not poisoned").push(compiled.clone()); // clone: the test inspects what was asked after the call
            let admitted = compiled.selection.selectors.iter().all(|selector| selector.ids.contains(&self.entity));
            let holds = compiled.selection.attrs.is_empty() || compiled.selection.attrs.iter().any(|attribute| self.attributes.contains(attribute));
            let rows = if admitted && holds { vec![self.entity.clone()] } else { Vec::new() }; // clone: the answer owns its identifier
            Box::pin(async move {
                Ok(QueryOutcome {
                    rows,
                    unapplied: ResidualFilter::default(),
                    pagination: PaginationOutcome::Applied { total: None },
                })
            })
        }
    }

    fn iri(value: &str) -> IriBuf {
        IriBuf::new(value.to_owned()).expect("a valid IRI")
    }

    fn vehicle() -> Identifier {
        "urn:ngsi-ld:Vehicle:1".parse().expect("a valid identifier")
    }

    fn probe(store: &Arc<AttributeIndex>) -> EntityStoreRegistrationProbe {
        EntityStoreRegistrationProbe::new(
            Arc::clone(store) as Arc<dyn EntityQueryStore>,
            JsonLdEngine::builder().allow_remote(false).build().expect("engine builds"),
            ContextTarget::Live,
        )
    }

    fn store() -> Arc<AttributeIndex> {
        Arc::new(AttributeIndex {
            entity: vehicle(),
            attributes: vec![iri(SPEED)],
            asked: Mutex::new(Vec::new()),
        })
    }

    /// Clause 5.9.2.4: an exclusive registration claiming an attribute the
    /// entity already holds finds it present, through one identifier query
    /// narrowed to that entity and those attributes.
    #[tokio::test]
    async fn an_attribute_the_entity_holds_is_present() {
        let store = store();
        let presence = probe(&store)
            .holds_any_attribute(&vehicle(), &[iri(BRAND), iri(SPEED)])
            .await
            .expect("the store answers");
        pretty_assertions::assert_eq!(presence, EntityPresence::Present);
        let asked = store.asked.lock().expect("the record is not poisoned");
        pretty_assertions::assert_eq!(asked.len(), 1);
        pretty_assertions::assert_eq!(asked[0].selection.selectors.len(), 1);
        pretty_assertions::assert_eq!(asked[0].selection.selectors[0].ids, vec![vehicle()]);
        pretty_assertions::assert_eq!(asked[0].selection.attrs, vec![iri(BRAND), iri(SPEED)]);
        pretty_assertions::assert_eq!(asked[0].pagination.limit, 1);
    }

    #[tokio::test]
    async fn an_attribute_the_entity_does_not_hold_is_absent() {
        let presence = probe(&store()).holds_any_attribute(&vehicle(), &[iri(BRAND)]).await.expect("the store answers");
        pretty_assertions::assert_eq!(presence, EntityPresence::Absent);
    }

    #[tokio::test]
    async fn an_entity_the_store_does_not_hold_holds_nothing() {
        let other: Identifier = "urn:ngsi-ld:Vehicle:2".parse().expect("a valid identifier");
        let presence = probe(&store()).holds_any_attribute(&other, &[iri(SPEED)]).await.expect("the store answers");
        pretty_assertions::assert_eq!(presence, EntityPresence::Absent);
    }

    /// No attribute named is no attribute held, and the store is not asked:
    /// an empty `attrs` would select the entity whatever it holds.
    #[tokio::test]
    async fn no_attribute_named_is_absent_without_asking_the_store() {
        let store = store();
        let presence = probe(&store).holds_any_attribute(&vehicle(), &[]).await.expect("answers");
        pretty_assertions::assert_eq!(presence, EntityPresence::Absent);
        assert!(store.asked.lock().expect("the record is not poisoned").is_empty());
    }
}
