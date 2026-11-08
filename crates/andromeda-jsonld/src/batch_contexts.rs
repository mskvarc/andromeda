//! The distinct `@context`s one batch names, each given its identity once.
//!
//! JSON-LD applies a top-level node's `@context` to that node alone, so every
//! entity of an `application/ld+json` batch is read under the context it
//! carried itself (clause 5.5.7: "the @context [...] provided by each API
//! call"), while every entity of an `application/json` batch shares the
//! request's `Link` context. Entities naming one and the same context are
//! resolved, admitted, prepared and forwarded together, so a batch whose
//! entities agree (the common case) does each of those once.
//!
//! Whether two entities name the same context is decided exactly once, when the
//! batch is parsed ([`BatchContexts`]), and recorded as a [`BatchContextId`].
//! Everything downstream groups by that identity ([`ContextGroups`]) and never
//! compares two contexts again.

use crate::{context_spec::ContextSpec, engine::processor::JsonLdEngine, error::JsonLdError, user_context_admission::screen_user_context};
use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_representation::context_terms::ContextTerms;
use derive_more::Deref;
use std::{sync::Arc, vec::IntoIter};

/// The identity of one distinct context a batch names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BatchContextId(usize);

/// One distinct context a batch names, with the identity it was given when
/// the batch was parsed.
#[derive(Debug, Clone)]
pub struct BatchContext {
    /// The identity every entity naming this context shares.
    id: BatchContextId,
    /// The context, shared by every entity naming it.
    spec: Arc<ContextSpec>,
}

impl BatchContext {
    /// The one context every entity of a batch shares: the one the request
    /// named, as an `application/json` batch names it in its `Link` header.
    #[must_use]
    pub fn shared(context: ContextSpec) -> BatchContext {
        BatchContext {
            id: BatchContextId(0),
            spec: Arc::new(context),
        }
    }

    /// The identity every entity naming this context shares.
    #[must_use]
    pub const fn id(&self) -> BatchContextId {
        self.id
    }

    /// The context itself.
    #[must_use]
    pub fn spec(&self) -> &ContextSpec {
        &self.spec
    }

    /// Admit the user `@context` one entity of a batch carried, as clause
    /// 5.5.7 admits a request's (see
    /// [`admit_user_context`](crate::user_context_admission::admit_user_context)).
    ///
    /// A context that loads and declares nothing clause 5.5.7 refuses, but
    /// does not process, is the failure of the entities naming it alone: the
    /// rest of the batch is read under the contexts its entities named.
    ///
    /// # Errors
    ///
    /// Returns the screening refusal of
    /// [`admit_user_context`](crate::user_context_admission::admit_user_context):
    /// a context graph that cycles or nests too deep, a context that does not
    /// resolve, or a JSON-LD Scoped Context.
    pub async fn admit(&self, engine: &JsonLdEngine) -> Result<EntityContextAdmission, JsonLdError> {
        screen_user_context(engine, &self.spec).await?;
        Ok(match engine.expansion_context(&self.spec).await {
            Ok(processed) => EntityContextAdmission::Admitted(self.admitted(processed.terms())),
            Err(unprocessable) => EntityContextAdmission::Unprocessable(unprocessable),
        })
    }

    /// Admit this context as the request's own, exactly as
    /// [`admit_user_context`](crate::user_context_admission::admit_user_context)
    /// admits a single request's: screened and then processed.
    ///
    /// A context the request named for every entity is no entity's failure:
    /// clause 5.5.6 raises `LdContextNotAvailable` for "a remote JSON-LD
    /// @context referenced by an incoming request" and `BadRequestData` for one
    /// that is not valid, so the request fails as a whole, before any entity is
    /// read under it.
    ///
    /// # Errors
    ///
    /// Returns the refusal of
    /// [`admit_user_context`](crate::user_context_admission::admit_user_context).
    pub async fn admit_for_request(&self, engine: &JsonLdEngine) -> Result<AdmittedBatchContext, JsonLdError> {
        screen_user_context(engine, &self.spec).await?;
        let processed = engine.expansion_context(&self.spec).await?;
        Ok(self.admitted(processed.terms()))
    }

    /// This context admitted, read through `terms`.
    fn admitted(&self, terms: &Arc<ContextTerms>) -> AdmittedBatchContext {
        AdmittedBatchContext {
            context: self.clone(),
            terms: Arc::clone(terms),
        }
    }
}

/// What the admission of the context one entity of a batch carried decided,
/// when the context passed the clause 5.5.7 screening.
#[derive(Debug)]
pub enum EntityContextAdmission {
    /// The context processes: every entity naming it is read under it.
    Admitted(AdmittedBatchContext),
    /// The context does not process, which every entity naming it is refused
    /// for.
    Unprocessable(JsonLdError),
}

/// A [`BatchContext`] admitted, with the aliases and containers it defines.
#[derive(Debug, Clone, Deref)]
pub struct AdmittedBatchContext {
    /// The admitted context.
    #[deref]
    context: BatchContext,
    /// The terms the processed context defines for core terms and the terms
    /// whose container reshapes what is written under them, which every entity
    /// naming it is read through.
    terms: Arc<ContextTerms>,
}

impl AdmittedBatchContext {
    /// The aliases and containers of the context, shared with the processed
    /// context they were read from.
    #[must_use]
    pub const fn terms(&self) -> &Arc<ContextTerms> {
        &self.terms
    }
}

/// The distinct contexts one batch's entities name, in the order each first
/// appears.
#[derive(Debug)]
pub struct BatchContexts {
    /// How the entities of the batch name their context.
    naming: ContextNaming,
}

/// How the entities of one batch name their context (clause 6.3.5).
#[derive(Debug)]
enum ContextNaming {
    /// Every entity carries its own `@context` member, as an
    /// `application/ld+json` batch does: each distinct written value, with the
    /// context it gave.
    PerEntity(Vec<(JsonLiteral, BatchContext)>),
    /// Every entity shares the one context the request named, as an
    /// `application/json` batch does.
    Shared(BatchContext),
}

impl BatchContexts {
    /// The contexts of a batch whose entities each carry their own `@context`
    /// member.
    #[must_use]
    pub const fn per_entity() -> BatchContexts {
        BatchContexts {
            naming: ContextNaming::PerEntity(Vec::new()),
        }
    }

    /// The contexts of a batch whose entities all share `context`, the one the
    /// request named.
    #[must_use]
    pub const fn shared(context: BatchContext) -> BatchContexts {
        BatchContexts {
            naming: ContextNaming::Shared(context),
        }
    }

    /// The context an entity is read under: the one the request named in a
    /// shared batch, or, in a batch whose entities carry their own, the one the
    /// written `@context` member `own` gives, which is the same [`BatchContext`]
    /// as every earlier entity that wrote an equal member.
    ///
    /// An entity of a shared batch carries no member of its own (clause 6.3.5
    /// refuses a body `@context` with `application/json` before this is
    /// reached), so `own` is only read for a per-entity batch; one that wrote
    /// none is read under the default context.
    pub fn context_of(&mut self, own: Option<JsonLiteral>) -> BatchContext {
        match &mut self.naming {
            ContextNaming::Shared(shared) => shared.clone(),
            ContextNaming::PerEntity(distinct) => {
                let written = own.unwrap_or_else(JsonLiteral::null);
                if let Some((_, context)) = distinct.iter().find(|(seen, _)| *seen == written) {
                    return context.clone();
                }
                let context = BatchContext {
                    id: BatchContextId(distinct.len()),
                    spec: Arc::new(ContextSpec::from_body(written.clone())), // clone: the written value stays the key later entities are compared with
                };
                distinct.push((written, context.clone()));
                context
            }
        }
    }

    /// The context the request as a whole is answered and forwarded under: the
    /// one every entity named, or the default context when the entities named
    /// several or none.
    #[must_use]
    pub fn request_context(self) -> ContextSpec {
        match self.naming {
            ContextNaming::Shared(shared) => Arc::unwrap_or_clone(shared.spec),
            ContextNaming::PerEntity(mut distinct) => match (distinct.pop(), distinct.is_empty()) {
                (Some((_, only)), true) => Arc::unwrap_or_clone(only.spec),
                (Some(_), false) | (None, _) => ContextSpec::none(),
            },
        }
    }
}

/// Items grouped by the context they are read under, in the order each
/// context first appears, keyed on the context's [`BatchContextId`].
#[derive(Debug)]
pub struct ContextGroups<T> {
    /// Each distinct context with the items naming it.
    groups: Vec<(BatchContext, Vec<T>)>,
}

impl<T> Default for ContextGroups<T> {
    // A manual impl because the derive would require `T: Default`, which an
    // empty grouping never needs.
    fn default() -> ContextGroups<T> {
        ContextGroups { groups: Vec::new() }
    }
}

impl<T> ContextGroups<T> {
    /// Add `item`, read under `context`, to the group of items naming the same
    /// context.
    pub fn push(&mut self, context: &BatchContext, item: T) {
        match self.groups.iter_mut().find(|(named, _)| named.id == context.id) {
            Some((_, items)) => items.push(item),
            None => self.groups.push((context.clone(), vec![item])),
        }
    }

    /// Each distinct context with the items naming it.
    pub fn iter(&self) -> impl Iterator<Item = (&BatchContext, &[T])> {
        self.groups.iter().map(|(context, items)| (context, items.as_slice()))
    }

    /// How many distinct contexts the items name.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.groups.len()
    }

    /// Whether no item was added.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }
}

impl<T> IntoIterator for ContextGroups<T> {
    type Item = (BatchContext, Vec<T>);
    type IntoIter = IntoIter<(BatchContext, Vec<T>)>;

    fn into_iter(self) -> Self::IntoIter {
        self.groups.into_iter()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        batch_contexts::{BatchContext, BatchContexts, ContextGroups, EntityContextAdmission},
        context_spec::ContextSpec,
        engine::processor::JsonLdEngine,
        test_documents::literal,
    };
    use iri_rs::IriBuf;
    use serde_json::json;
    use std::borrow::Cow;

    /// Entities writing equal `@context` members share one identity, whatever
    /// the allocation; different members get different ones, in first-seen
    /// order.
    #[test]
    fn equal_written_contexts_share_one_identity() {
        let mut contexts = BatchContexts::per_entity();
        let a = contexts.context_of(Some(literal(&json!({"speed": "https://a.example/speed"}))));
        let b = contexts.context_of(Some(literal(&json!({"speed": "https://b.example/speed"}))));
        let a_again = contexts.context_of(Some(literal(&json!({"speed": "https://a.example/speed"}))));

        pretty_assertions::assert_eq!(a.id(), a_again.id());
        assert_ne!(a.id(), b.id());
        pretty_assertions::assert_eq!(a.spec(), &ContextSpec::from_body(literal(&json!({"speed": "https://a.example/speed"}))));
    }

    /// Every entity of a shared batch is read under the request's context.
    #[test]
    fn a_shared_batch_reads_every_entity_under_the_request_context() {
        let link = ContextSpec::from_link(IriBuf::new("https://example.com/ctx.jsonld".to_owned()).expect("a valid IRI"));
        let mut contexts = BatchContexts::shared(BatchContext::shared(link.clone()));
        let first = contexts.context_of(None);
        let second = contexts.context_of(None);

        pretty_assertions::assert_eq!(first.id(), second.id());
        pretty_assertions::assert_eq!(first.spec(), &link);
        pretty_assertions::assert_eq!(contexts.request_context(), link);
    }

    /// The request is answered under the one context its entities agree on,
    /// and under the default context when they name several.
    #[test]
    fn the_request_context_is_the_one_every_entity_named_or_the_default() {
        let mut agreeing = BatchContexts::per_entity();
        agreeing.context_of(Some(literal(&json!("https://example.com/a.jsonld"))));
        agreeing.context_of(Some(literal(&json!("https://example.com/a.jsonld"))));
        pretty_assertions::assert_eq!(
            agreeing.request_context(),
            ContextSpec::from_body(literal(&json!("https://example.com/a.jsonld")))
        );

        let mut differing = BatchContexts::per_entity();
        differing.context_of(Some(literal(&json!("https://example.com/a.jsonld"))));
        differing.context_of(Some(literal(&json!("https://example.com/b.jsonld"))));
        pretty_assertions::assert_eq!(differing.request_context(), ContextSpec::none());

        pretty_assertions::assert_eq!(BatchContexts::per_entity().request_context(), ContextSpec::none());
    }

    /// The engine every admission below runs on, which retrieves no remote
    /// document.
    fn offline_engine() -> JsonLdEngine {
        JsonLdEngine::builder().allow_remote(false).build().expect("the offline engine builds")
    }

    /// Clause 5.5.6: a remote `@context` the request names that cannot be
    /// retrieved is `LdContextNotAvailable` for the request.
    #[tokio::test]
    async fn a_request_context_that_cannot_be_retrieved_is_not_available() {
        let link = ContextSpec::from_link(IriBuf::new("https://example.com/ctx.jsonld".to_owned()).expect("a valid IRI"));
        let error = BatchContext::shared(link)
            .admit_for_request(&offline_engine())
            .await
            .expect_err("the context cannot be retrieved");
        assert!(error.is_context_not_available(), "{error}");
    }

    /// Clause 5.5.6: a request context that loads but is not valid is refused
    /// for the request, while an entity's own context that does not process is
    /// the failure of the entities naming it.
    #[tokio::test]
    async fn a_request_context_that_does_not_process_is_refused_for_the_request() {
        let invalid = BatchContext::shared(ContextSpec::from_body(literal(&json!({"speed": {"@id": 5}}))));
        let engine = offline_engine();

        let entity = invalid.admit(&engine).await.expect("the context passes the screening");
        assert!(matches!(entity, EntityContextAdmission::Unprocessable(_)), "{entity:?}");
        let error = invalid.admit_for_request(&engine).await.expect_err("the context does not process");
        assert!(!error.is_context_not_available(), "{error}");
    }

    /// An entity's context that processes is admitted with the terms it
    /// defines for core terms.
    #[tokio::test]
    async fn an_entity_context_that_processes_carries_its_aliases() {
        let own = BatchContexts::per_entity().context_of(Some(literal(&json!({"kind": "@type"}))));
        let EntityContextAdmission::Admitted(admitted) = own.admit(&offline_engine()).await.expect("the context is admitted") else {
            panic!("the context processes");
        };
        pretty_assertions::assert_eq!(admitted.terms().aliases().core_name(Cow::Borrowed("kind")), "type");
        pretty_assertions::assert_eq!(admitted.id(), own.id());
    }

    /// A request context that processes is admitted for every entity under its
    /// own identity.
    #[tokio::test]
    async fn a_request_context_that_processes_is_admitted() {
        let shared = BatchContext::shared(ContextSpec::from_body(literal(&json!({"speed": "https://example.org/speed"}))));
        let admitted = shared.admit_for_request(&offline_engine()).await.expect("the context processes");
        pretty_assertions::assert_eq!(admitted.id(), shared.id());
    }

    /// Items are grouped by the identity of the context they name, in
    /// first-seen order.
    #[test]
    fn items_are_grouped_by_the_identity_of_their_context() {
        let mut contexts = BatchContexts::per_entity();
        let a = contexts.context_of(Some(literal(&json!({"speed": "https://a.example/speed"}))));
        let b = contexts.context_of(Some(literal(&json!({"speed": "https://b.example/speed"}))));
        let a_again = contexts.context_of(Some(literal(&json!({"speed": "https://a.example/speed"}))));
        let mut groups = ContextGroups::default();

        groups.push(&a, 0);
        groups.push(&b, 1);
        groups.push(&a_again, 2);
        groups.push(&a, 3);

        pretty_assertions::assert_eq!(groups.len(), 2);
        pretty_assertions::assert_eq!(groups.iter().map(|(_, items)| items.to_vec()).collect::<Vec<_>>(), vec![vec![0, 2, 3], vec![1]]);
    }
}
