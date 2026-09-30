//! The `@context` a response or a notification names.
//!
//! Clause 5.2.3 has an `application/ld+json` Entity, Context Source
//! Registration, Subscription or Notification carry "an array (flattened to a
//! single string if necessary), containing a user @context where present, and
//! the core @context", and clause 5.5.7 makes that user `@context` the one the
//! call provided. Clause 4.4 adds that "when rendering NGSI-LD Elements, the
//! Core @context shall always be treated as if it had been originally placed in
//! the last position, so that [...] upstream JSON-LD processors can properly
//! expand as NGSI-LD": a standard processor reads the `@context` a response
//! names in order and knows nothing of the NGSI-LD precedence rule, so the core
//! context has to be the last context that value leaves in force. Annex C.8
//! answers a client whose wrapper `@context` already ends with the core context
//! with that wrapper alone, and the ETSI suite's own compound context is such a
//! wrapper.
//!
//! [`PresentedContext`] applies all of that once, for every rendering: the
//! request's contexts in order, repeats included, every core reference the
//! request named removed, and the v1.9 core context appended last unless what
//! remains provably leaves it in force last already. Whether it does is a fact
//! about the documents the request named ([`ContextTail`]), resolved through
//! [`ContextTailLookup`] exactly as processing resolves them, so the answer is
//! the same whichever cache tier holds a document at the time. Only a reference
//! cycle, or a chain deeper than [`MAX_CONTEXT_REFERENCE_DEPTH`], is taken as
//! not ending with the core, which appends it; appending it after a document
//! that already ends with it names the same protected definitions twice, which
//! JSON-LD 1.1 accepts. Processing refuses either before a response is
//! rendered ([`crate::context_graph`]), so the guard is never reached for a
//! request that was processed.
//!
//! An `application/json` response names its context in exactly one `Link`
//! header instead (clause 6.3.6; JSON-LD 1.1 section 6.1). That link names the
//! same presented value and nothing else ([`PresentedContext::link_target`]):
//! the value itself when it is one string, and otherwise a hosted wrapper whose
//! `@context` is the value (Annex C.8).

use crate::{
    context_graph::MAX_CONTEXT_REFERENCE_DEPTH,
    context_spec::ContextSpec,
    context_tail::ContextTail,
    core_context_precedence::{CoreImport, drop_core_import, is_core_context_json_string},
    error::JsonLdError,
    hosted_context_recognizer::{HostedContextRecognizer, NoHostedContexts},
    loader::http::HttpContextLoader,
};
use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
use iri_rs::{Iri, IriBuf};
use serde_json::Value;
use std::{
    collections::HashSet,
    future::{Future, ready},
};

/// Where the facts about the documents a request named are read from.
pub trait ContextTailLookup: Sync {
    /// What the context document at `url` leaves in force last.
    ///
    /// # Errors
    ///
    /// Returns the [`JsonLdError`] the document cannot be loaded with, which
    /// the request would have failed with had processing loaded it.
    fn tail_of(&self, url: &IriBuf) -> impl Future<Output = Result<ContextTail, JsonLdError>> + Send;

    /// What recognises a root-relative reference to one of the tenant's own
    /// stored `@context`s, as processing recognises it.
    fn hosted_contexts(&self) -> &dyn HostedContextRecognizer;
}

impl ContextTailLookup for HttpContextLoader {
    fn tail_of(&self, url: &IriBuf) -> impl Future<Output = Result<ContextTail, JsonLdError>> + Send {
        self.load_tail(url)
    }

    fn hosted_contexts(&self) -> &dyn HostedContextRecognizer {
        HttpContextLoader::hosted_contexts(self)
    }
}

/// A lookup that holds no document.
///
/// A test double: every broker operation resolves through its tenant's
/// loader. Every document is reported as not ending with the core context,
/// which appends it.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoLoadedContexts;

impl ContextTailLookup for NoLoadedContexts {
    fn tail_of(&self, _url: &IriBuf) -> impl Future<Output = Result<ContextTail, JsonLdError>> + Send {
        ready(Ok(ContextTail::Unresolvable))
    }

    fn hosted_contexts(&self) -> &dyn HostedContextRecognizer {
        &NoHostedContexts
    }
}

/// The `@context` one response names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentedContext {
    /// The value an `application/ld+json` body carries as its `@context`,
    /// and the one an `application/json` response's `Link` header names.
    value: Value,
}

/// What the one JSON-LD context `Link` header of a response names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextLinkTarget {
    /// The presented value is one absolute URL, which the link names as is.
    Reference(IriBuf),
    /// The presented value is several contexts, or one written inline, which
    /// no single URL names: the link names a hosted wrapper whose stored
    /// document is exactly `{"@context": <value>}` (clause 5.13.1
    /// `ImplicitlyCreated`, Annex C.8).
    Hosted(Value),
}

impl PresentedContext {
    /// The `@context` of a response to a request that supplied `spec`.
    ///
    /// # Errors
    ///
    /// Returns the [`JsonLdError`] a document the request named cannot be
    /// loaded with (`LdContextNotAvailable`, clause 5.5.6).
    pub async fn resolve(spec: &ContextSpec, documents: &impl ContextTailLookup) -> Result<PresentedContext, JsonLdError> {
        let request_contexts = spec.request_context().entries();
        Ok(PresentedContext {
            value: RetainedContexts::of(request_contexts).into_value(documents).await?,
        })
    }

    /// The `@context` of a resource that always answers under the core context
    /// alone.
    #[must_use]
    pub fn core() -> PresentedContext {
        PresentedContext { value: core_context_value() }
    }

    /// The value an `application/ld+json` body carries as its `@context`.
    #[must_use]
    pub const fn body(&self) -> &Value {
        &self.value
    }

    /// Consume the presented context and take its value.
    #[must_use]
    pub fn into_value(self) -> Value {
        self.value
    }

    /// What the one `Link` header of an `application/json` response names
    /// (clause 6.3.6), derived from the presented value and nothing else.
    ///
    /// One absolute URL names itself: the core context when the request named
    /// nothing, v1.9 when it named only a core release, the client's context
    /// when that provably ends with v1.9. Anything else is a hosted wrapper of
    /// exactly the presented value, so a generic JSON-LD processor following
    /// the link reads the core context last as clause 4.4 requires; Annex C.8
    /// names that case, "when the Core @context has not been previously
    /// included by the wrapper @context".
    #[must_use]
    pub fn link_target(&self) -> ContextLinkTarget {
        match &self.value {
            Value::String(reference) => match Iri::parse(reference.as_str()) {
                Ok(absolute) => ContextLinkTarget::Reference(IriBuf::from(absolute)),
                Err(_) => ContextLinkTarget::Hosted(self.value.clone()), // clone: the wrapper document owns its copy of the value
            },
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_) | Value::Object(_) => ContextLinkTarget::Hosted(self.value.clone()), // clone: same
        }
    }
}

/// The `@context` an `application/ld+json` or `application/geo+json`
/// notification body names (clauses 5.8.1.4, 5.8.6 and 5.2.3).
///
/// The notification context "shall be the one specified in the jsonldContext
/// field", and clause 5.2.3 still asks the Notification to carry the core
/// context beside it, so the same rule as for a response applies to that one
/// context. A `jsonldContext` that names a core context itself is the
/// subscriber's explicit, stored choice and is named alone as written: it
/// leaves a core context in force last already, and appending v1.9 after a
/// protected v1.8 would be a `protected term redefinition` to every JSON-LD 1.1
/// processor. No `jsonldContext` at all leaves the core context alone
/// (clause 5.5.5).
///
/// # Errors
///
/// Returns the [`JsonLdError`] the `jsonldContext` document cannot be loaded
/// with.
pub async fn notification_body_context(jsonld_context: Option<&IriBuf>, documents: &impl ContextTailLookup) -> Result<Value, JsonLdError> {
    match jsonld_context {
        Some(context) if is_core_context_json_string(context.as_str()) => Ok(Value::String(context.as_str().to_owned())),
        Some(context) => {
            RetainedContexts::of(vec![Value::String(context.as_str().to_owned())])
                .into_value(documents)
                .await
        }
        None => Ok(core_context_value()),
    }
}

/// The contexts a request named, in the order it named them, once every core
/// reference among them is taken out.
struct RetainedContexts {
    /// The request's own contexts in order, repeats included, core references
    /// removed.
    kept: Vec<Value>,
    /// Whether the request named a core context itself.
    named_core: NamedCore,
}

impl RetainedContexts {
    /// Take every core reference out of the contexts a request named, directly
    /// or through an `@import`.
    ///
    /// Repeats are kept: JSON-LD processes an `@context` array in order, so
    /// `[A, B, A]` leaves A's definitions in force where `[A, B]` leaves B's,
    /// and the response names the context the call provided (clause 5.5.7).
    fn of(request_contexts: Vec<Value>) -> RetainedContexts {
        let mut named_core = NamedCore::No;
        let mut kept = Vec::with_capacity(request_contexts.len() + 1);
        for context in request_contexts {
            match context {
                Value::String(reference) if is_core_context_json_string(&reference) => named_core = NamedCore::Yes,
                Value::String(reference) => kept.push(Value::String(reference)),
                Value::Object(mut definition) => {
                    if drop_core_import(&mut definition) == CoreImport::Dropped {
                        named_core = NamedCore::Yes;
                    }
                    if !definition.is_empty() {
                        kept.push(Value::Object(definition));
                    }
                }
                other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_)) => kept.push(other),
            }
        }
        RetainedContexts { kept, named_core }
    }

    /// The presented value for these contexts: the v1.9 core context appended
    /// last unless what remains provably leaves it in force last already, and
    /// a single context written as the one string it is.
    async fn into_value(self, documents: &impl ContextTailLookup) -> Result<Value, JsonLdError> {
        let RetainedContexts { mut kept, named_core } = self;
        let ends_with_core = match (named_core, kept.last()) {
            (NamedCore::Yes, _) | (NamedCore::No, None) => CoreInForce::NotLast,
            (NamedCore::No, Some(last)) => core_in_force_after(last, documents).await?,
        };
        if ends_with_core == CoreInForce::NotLast {
            kept.push(core_context_value());
        }
        Ok(match <[Value; 1]>::try_from(kept) {
            Ok([single]) => single,
            Err(several) => Value::Array(several),
        })
    }
}

/// Whether the request itself named a core context, which the response
/// replaces by the v1.9 one, last.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NamedCore {
    /// It named one, directly or through an `@import`.
    Yes,
    /// It named none.
    No,
}

/// Whether a context leaves the v1.9 core context in force last.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CoreInForce {
    /// The last context it processes is the v1.9 core context.
    Last,
    /// It does not, or the chain could not be followed to its end.
    NotLast,
}

/// Whether `context`, the last context a request named, leaves the v1.9 core
/// context in force last, following the documents it references.
async fn core_in_force_after(context: &Value, documents: &impl ContextTailLookup) -> Result<CoreInForce, JsonLdError> {
    let mut tail = ContextTail::of_request_context(context, documents.hosted_contexts());
    let mut followed = HashSet::new();
    loop {
        match tail {
            ContextTail::CurrentCore => return Ok(CoreInForce::Last),
            // A relative tail is resolved by the lookup against the document it
            // was read from; one that reaches here has no document to resolve
            // against and cannot be followed.
            ContextTail::OtherCore | ContextTail::LocalDefinitions | ContextTail::Relative(_) | ContextTail::Unresolvable => {
                return Ok(CoreInForce::NotLast);
            }
            ContextTail::Reference(url) => {
                if followed.len() >= MAX_CONTEXT_REFERENCE_DEPTH || !followed.insert(url.clone()) {
                    // clone: the set remembers the reference the lookup below reads
                    return Ok(CoreInForce::NotLast);
                }
                tail = documents.tail_of(&url).await?;
            }
        }
    }
}

/// The v1.9 core context, as a `@context` value names it.
fn core_context_value() -> Value {
    Value::String(NGSI_LD_CORE_CONTEXT_URL.as_str().to_owned())
}

#[cfg(test)]
mod tests {
    use crate::{
        context_spec::ContextSpec,
        context_tail::ContextTail,
        core_context::{NGSI_LD_CORE_CONTEXT_V13_URL, NGSI_LD_CORE_CONTEXT_V16_URL, NGSI_LD_CORE_CONTEXT_V18_URL},
        error::JsonLdError,
        hosted_context_recognizer::{HostedContextRecognizer, HostedReference},
        presented_context::{ContextLinkTarget, ContextTailLookup, NoLoadedContexts, PresentedContext, notification_body_context},
    };
    use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
    use iri_rs::{IriBuf, IriRefBuf};
    use serde_json::{Value, json};
    use std::{
        collections::HashMap,
        future::{Future, ready},
        sync::atomic::{AtomicUsize, Ordering},
    };

    /// The suite-shaped compound context: a user context ending with v1.9.
    const COMPOUND: &str = "https://example.com/compound.jsonld";
    /// A user context that names no core context.
    const USER: &str = "https://example.com/user.jsonld";

    /// Documents whose tails are fixed in advance; any other document fails to
    /// load, as an unreachable one does. Every load is counted.
    #[derive(Debug, Default)]
    struct KnownTails {
        tails: HashMap<IriBuf, ContextTail>,
        loads: AtomicUsize,
    }

    impl KnownTails {
        fn with(mut self, url: &str, tail: ContextTail) -> KnownTails {
            self.tails.insert(iri(url), tail);
            self
        }

        fn loads(&self) -> usize {
            self.loads.load(Ordering::SeqCst)
        }
    }

    impl ContextTailLookup for KnownTails {
        fn tail_of(&self, url: &IriBuf) -> impl Future<Output = Result<ContextTail, JsonLdError>> + Send {
            self.loads.fetch_add(1, Ordering::SeqCst);
            ready(
                self.tails
                    .get(url)
                    .cloned()
                    .ok_or_else(|| JsonLdError::ContextFetchDisabled { url: url.clone() }),
            )
        }

        fn hosted_contexts(&self) -> &dyn HostedContextRecognizer {
            &StoredPaths
        }
    }

    /// Recognises a root-relative stored-context path as the identifier it
    /// names.
    #[derive(Debug)]
    struct StoredPaths;

    impl HostedContextRecognizer for StoredPaths {
        fn recognize(&self, reference: &IriRefBuf) -> HostedReference {
            reference
                .as_str()
                .strip_prefix("/ngsi-ld/v1/jsonldContexts/")
                .and_then(|identifier| identifier.parse().ok())
                .map_or(HostedReference::Unrelated, HostedReference::Own)
        }
    }

    fn iri(url: &str) -> IriBuf {
        IriBuf::new(url.to_owned()).expect("a valid URL")
    }

    fn documents() -> KnownTails {
        KnownTails::default()
            .with(COMPOUND, ContextTail::CurrentCore)
            .with(USER, ContextTail::LocalDefinitions)
            .with(STORED_WRAPPER, ContextTail::CurrentCore)
    }

    /// A user context written as `["<core v1.9>", {"@vocab": ...}]`.
    const CORE_THEN_VOCAB: &str = "https://example.com/core-then-vocab.jsonld";
    /// A user context document that imports v1.8 beside its own terms.
    const IMPORTS_V18: &str = "https://example.com/imports-v18.jsonld";
    /// A user context document ending with v1.8.
    const ENDS_WITH_V18: &str = "https://example.com/ends-with-v18.jsonld";
    /// A user context document whose last entry is a relative reference.
    const RELATIVE_TAIL: &str = "https://example.com/contexts/relative-tail.jsonld";
    /// What that relative reference resolves to against its document's URL.
    const RESOLVED_WRAPPER: &str = "https://example.com/contexts/wrapper.jsonld";

    /// A stored wrapper the tenant holds, which ends with the core context.
    const STORED_WRAPPER: &str = "urn:ngsi-ld:JsonLdContext:6f1c0e2a-3b5d-4e7f-8a9b-0c1d2e3f4a5b";

    /// A request body has no base URL, so a root-relative reference to one of
    /// the tenant's own stored contexts is followed through the identifier it
    /// names, as processing follows it: a stored wrapper ending with the core
    /// context is named alone.
    #[tokio::test]
    async fn a_root_relative_reference_to_a_stored_wrapper_is_followed_through_storage() {
        let relative = format!("/ngsi-ld/v1/jsonldContexts/{STORED_WRAPPER}");

        pretty_assertions::assert_eq!(body_for(json!(relative)).await, json!(relative));
        pretty_assertions::assert_eq!(
            PresentedContext::resolve(&ContextSpec::from_body(json!(relative)), &NoLoadedContexts)
                .await
                .expect("no document is loaded")
                .into_value(),
            json!([relative, core()]),
            "without the tenant's recogniser the reference cannot be followed, so the core context is appended"
        );
    }

    async fn presented(spec: &ContextSpec) -> PresentedContext {
        PresentedContext::resolve(spec, &documents()).await.expect("every named document loads")
    }

    async fn body_for(body_context: Value) -> Value {
        presented(&ContextSpec::from_body(body_context)).await.into_value()
    }

    async fn body_for_link(url: &str) -> Value {
        presented(&ContextSpec::from_link(iri(url))).await.into_value()
    }

    async fn target_for(body_context: Value) -> ContextLinkTarget {
        presented(&ContextSpec::from_body(body_context)).await.link_target()
    }

    fn core() -> Value {
        json!(NGSI_LD_CORE_CONTEXT_URL.as_str())
    }

    fn reference(url: &str) -> ContextLinkTarget {
        ContextLinkTarget::Reference(iri(url))
    }

    /// Clause 5.5.5 and Annex C.7: with no context of its own, a request is
    /// answered under the core context alone, written as one string.
    #[tokio::test]
    async fn a_request_naming_no_context_is_answered_under_the_core_context_alone() {
        let presented = presented(&ContextSpec::none()).await;

        pretty_assertions::assert_eq!(presented.body(), &core());
        pretty_assertions::assert_eq!(presented.link_target(), reference(NGSI_LD_CORE_CONTEXT_URL.as_str()));
    }

    /// Annex C.8: a wrapper that already ends with the core context is named
    /// alone, as the one string it is.
    #[tokio::test]
    async fn a_context_ending_with_the_core_context_is_named_alone() {
        pretty_assertions::assert_eq!(body_for_link(COMPOUND).await, json!(COMPOUND));
        pretty_assertions::assert_eq!(body_for(json!(COMPOUND)).await, json!(COMPOUND));
        pretty_assertions::assert_eq!(body_for(json!([COMPOUND])).await, json!(COMPOUND));
    }

    /// Clause 5.2.3: a user context that does not reach the core context is
    /// followed by it.
    #[tokio::test]
    async fn a_context_that_does_not_end_with_the_core_context_is_followed_by_it() {
        pretty_assertions::assert_eq!(body_for_link(USER).await, json!([USER, core()]));
        pretty_assertions::assert_eq!(
            body_for(json!({ "speed": "https://example.com/speed" })).await,
            json!([{ "speed": "https://example.com/speed" }, core()])
        );
    }

    /// Clause 4.4: the core context a request named, of any release and in
    /// any position, is answered as the v1.9 core context, last.
    #[tokio::test]
    async fn a_core_context_the_request_named_is_answered_as_the_current_one_last() {
        pretty_assertions::assert_eq!(body_for(json!([USER, NGSI_LD_CORE_CONTEXT_V16_URL.as_str()])).await, json!([USER, core()]));
        pretty_assertions::assert_eq!(body_for(json!([NGSI_LD_CORE_CONTEXT_V18_URL.as_str(), USER])).await, json!([USER, core()]));
        pretty_assertions::assert_eq!(body_for(json!(NGSI_LD_CORE_CONTEXT_V18_URL.as_str())).await, core());
        pretty_assertions::assert_eq!(body_for(json!([COMPOUND, NGSI_LD_CORE_CONTEXT_URL.as_str()])).await, json!([COMPOUND, core()]));
    }

    /// A repeated v1.9 core context is a core reference like any other and is
    /// removed wherever it stands.
    #[tokio::test]
    async fn a_repeated_current_core_context_is_removed_and_named_once_last() {
        pretty_assertions::assert_eq!(body_for(json!([core(), USER, core()])).await, json!([USER, core()]));
    }

    /// JSON-LD 1.1 section 4.1.10: an `@import`ed core context is a core
    /// reference like any other.
    #[tokio::test]
    async fn an_imported_core_context_is_answered_as_the_current_one_last() {
        pretty_assertions::assert_eq!(
            body_for(json!({ "@import": NGSI_LD_CORE_CONTEXT_V18_URL.as_str(), "speed": "https://example.com/speed" })).await,
            json!([{ "speed": "https://example.com/speed" }, core()])
        );
        pretty_assertions::assert_eq!(body_for(json!({ "@import": NGSI_LD_CORE_CONTEXT_V13_URL.as_str() })).await, core());
    }

    /// Clause 5.5.7 and JSON-LD 1.1 section 4.1.2: `[A, B, A]` leaves A's
    /// definitions in force, so the repeat is part of the context the call
    /// provided and is kept.
    #[tokio::test]
    async fn repeated_request_contexts_are_kept_in_order() {
        pretty_assertions::assert_eq!(body_for(json!([USER, COMPOUND, USER])).await, json!([USER, COMPOUND, USER, core()]));
        pretty_assertions::assert_eq!(body_for(json!([USER, USER])).await, json!([USER, USER, core()]));
    }

    /// Clause 5.5.6: a document the tail lookup cannot load fails the request
    /// as `LdContextNotAvailable`, exactly as processing it would have.
    #[tokio::test]
    async fn a_document_that_cannot_be_loaded_is_context_not_available() {
        let spec = ContextSpec::from_link(iri("https://example.com/unreachable.jsonld"));

        let error = PresentedContext::resolve(&spec, &documents()).await.expect_err("the document cannot be loaded");

        assert!(error.is_context_not_available(), "unexpected error: {error}");
    }

    /// The tail of the last named document is read once through the lookup,
    /// which loads it when no cache holds it.
    #[tokio::test]
    async fn the_last_named_document_is_loaded_once_to_read_its_tail() {
        let lookup = documents();
        let spec = ContextSpec::from_link(iri(COMPOUND));

        let presented = PresentedContext::resolve(&spec, &lookup).await.expect("the document loads");

        pretty_assertions::assert_eq!(presented.body(), &json!(COMPOUND));
        pretty_assertions::assert_eq!(lookup.loads(), 1);
    }

    #[tokio::test]
    async fn a_chain_of_references_is_followed_to_the_document_that_ends_it() {
        let chained = KnownTails::default()
            .with("https://example.com/outer.jsonld", ContextTail::Reference(iri(COMPOUND)))
            .with(COMPOUND, ContextTail::CurrentCore);
        let spec = ContextSpec::from_link(iri("https://example.com/outer.jsonld"));

        let presented = PresentedContext::resolve(&spec, &chained).await.expect("every document loads");

        pretty_assertions::assert_eq!(presented.body(), &json!("https://example.com/outer.jsonld"));
        pretty_assertions::assert_eq!(chained.loads(), 2);
    }

    /// A reference cycle never ends with the core context, so it is appended.
    #[tokio::test]
    async fn a_cycle_of_references_is_followed_by_the_core_context() {
        let cyclic = KnownTails::default()
            .with("https://example.com/a.jsonld", ContextTail::Reference(iri("https://example.com/b.jsonld")))
            .with("https://example.com/b.jsonld", ContextTail::Reference(iri("https://example.com/a.jsonld")));
        let spec = ContextSpec::from_link(iri("https://example.com/a.jsonld"));

        let presented = PresentedContext::resolve(&spec, &cyclic).await.expect("every document loads");

        pretty_assertions::assert_eq!(presented.body(), &json!(["https://example.com/a.jsonld", core()]));
    }

    /// The test double that holds no document appends the core context.
    #[tokio::test]
    async fn a_lookup_holding_no_document_appends_the_core_context() {
        let spec = ContextSpec::from_link(iri(COMPOUND));

        let presented = PresentedContext::resolve(&spec, &NoLoadedContexts).await.expect("nothing is loaded");

        pretty_assertions::assert_eq!(presented.body(), &json!([COMPOUND, core()]));
    }

    /// Clause 6.3.6 and Annex C.8: a presented value that is one string is
    /// linked as itself: the core context for a request naming none or only a
    /// core release, the client's context when it ends with the core.
    #[tokio::test]
    async fn a_presented_single_string_is_linked_as_itself() {
        pretty_assertions::assert_eq!(
            presented(&ContextSpec::none()).await.link_target(),
            reference(NGSI_LD_CORE_CONTEXT_URL.as_str())
        );
        pretty_assertions::assert_eq!(
            presented(&ContextSpec::from_link(iri(NGSI_LD_CORE_CONTEXT_V16_URL.as_str())))
                .await
                .link_target(),
            reference(NGSI_LD_CORE_CONTEXT_URL.as_str())
        );
        pretty_assertions::assert_eq!(
            target_for(json!(NGSI_LD_CORE_CONTEXT_V18_URL.as_str())).await,
            reference(NGSI_LD_CORE_CONTEXT_URL.as_str())
        );
        pretty_assertions::assert_eq!(presented(&ContextSpec::from_link(iri(COMPOUND))).await.link_target(), reference(COMPOUND));
    }

    /// A core context named beside the client's own is replaced by v1.9 last,
    /// so the presented value is two contexts and is hosted.
    #[tokio::test]
    async fn a_core_context_named_beside_the_clients_own_is_linked_through_a_wrapper() {
        pretty_assertions::assert_eq!(
            target_for(json!([COMPOUND, NGSI_LD_CORE_CONTEXT_V18_URL.as_str()])).await,
            ContextLinkTarget::Hosted(json!([COMPOUND, core()]))
        );
    }

    /// Annex C.8: a single user context that does not end with the core is
    /// linked through a hosted wrapper holding it and the core context, so a
    /// generic processor following the link reads the core last (clause 4.4).
    #[tokio::test]
    async fn a_single_context_not_ending_with_the_core_is_linked_through_a_wrapper_of_the_body_value() {
        pretty_assertions::assert_eq!(
            presented(&ContextSpec::from_link(iri(USER))).await.link_target(),
            ContextLinkTarget::Hosted(json!([USER, core()]))
        );
        pretty_assertions::assert_eq!(target_for(json!([USER])).await, ContextLinkTarget::Hosted(json!([USER, core()])));
    }

    /// JSON-LD 1.1 section 6.1 admits one context link, so several contexts or
    /// one written inline are linked through a hosted wrapper of exactly the
    /// body value.
    #[tokio::test]
    async fn several_contexts_or_an_inline_one_are_linked_through_a_wrapper_of_the_body_value() {
        pretty_assertions::assert_eq!(target_for(json!([USER, COMPOUND])).await, ContextLinkTarget::Hosted(json!([USER, COMPOUND])));
        pretty_assertions::assert_eq!(
            target_for(json!({ "speed": "https://example.com/speed" })).await,
            ContextLinkTarget::Hosted(json!([{ "speed": "https://example.com/speed" }, core()]))
        );
        let inline_beside_user = presented(&ContextSpec::from_body(json!([USER, { "speed": "https://example.com/speed" }]))).await;
        pretty_assertions::assert_eq!(inline_beside_user.link_target(), ContextLinkTarget::Hosted(inline_beside_user.body().clone()));
    }

    /// A user context written as `["<core v1.9>", {"@vocab": ...}]` imports
    /// the core first and then sets its own `@vocab`, which a standard
    /// processor keeps in force after the core; the core context is therefore
    /// appended so the response names what the broker processed, core last
    /// (clause 4.4).
    #[tokio::test]
    async fn a_user_context_importing_the_core_before_its_own_vocab_is_followed_by_the_core() {
        let lookup = KnownTails::default().with(CORE_THEN_VOCAB, ContextTail::LocalDefinitions);
        let presented = PresentedContext::resolve(&ContextSpec::from_link(iri(CORE_THEN_VOCAB)), &lookup)
            .await
            .expect("the document loads");

        pretty_assertions::assert_eq!(presented.body(), &json!([CORE_THEN_VOCAB, core()]));
        pretty_assertions::assert_eq!(presented.link_target(), ContextLinkTarget::Hosted(json!([CORE_THEN_VOCAB, core()])));
    }

    /// A user context document that imports v1.8 is processed with v1.9, and
    /// what it leaves in force last is its own definitions, so it is named
    /// with the v1.9 core context after it.
    #[tokio::test]
    async fn a_user_context_importing_an_older_core_is_followed_by_the_current_core() {
        let lookup = KnownTails::default().with(IMPORTS_V18, ContextTail::LocalDefinitions);
        let presented = PresentedContext::resolve(&ContextSpec::from_link(iri(IMPORTS_V18)), &lookup)
            .await
            .expect("the document loads");

        pretty_assertions::assert_eq!(presented.body(), &json!([IMPORTS_V18, core()]));
    }

    /// A user document ending with v1.8 leaves another release of the core in
    /// force last, so v1.9 is appended after it.
    #[tokio::test]
    async fn a_user_context_ending_with_an_older_core_is_followed_by_the_current_core() {
        let lookup = KnownTails::default().with(ENDS_WITH_V18, ContextTail::OtherCore);
        let presented = PresentedContext::resolve(&ContextSpec::from_link(iri(ENDS_WITH_V18)), &lookup)
            .await
            .expect("the document loads");

        pretty_assertions::assert_eq!(presented.body(), &json!([ENDS_WITH_V18, core()]));
        pretty_assertions::assert_eq!(presented.link_target(), ContextLinkTarget::Hosted(json!([ENDS_WITH_V18, core()])));
    }

    /// A user document whose last entry is a relative reference to a document
    /// ending with v1.9 is followed through the resolved reference (the lookup
    /// resolves it against the document's URL) and named alone.
    #[tokio::test]
    async fn a_user_context_ending_with_a_relative_reference_to_a_core_ending_wrapper_is_named_alone() {
        let lookup = KnownTails::default()
            .with(RELATIVE_TAIL, ContextTail::Reference(iri(RESOLVED_WRAPPER)))
            .with(RESOLVED_WRAPPER, ContextTail::CurrentCore);
        let presented = PresentedContext::resolve(&ContextSpec::from_link(iri(RELATIVE_TAIL)), &lookup)
            .await
            .expect("every document loads");

        pretty_assertions::assert_eq!(presented.body(), &json!(RELATIVE_TAIL));
        pretty_assertions::assert_eq!(presented.link_target(), reference(RELATIVE_TAIL));
    }

    /// A reference cycle is linked through a wrapper holding the cycle's entry
    /// and the core context.
    #[tokio::test]
    async fn a_cycle_of_references_is_linked_through_a_wrapper() {
        let cyclic = KnownTails::default()
            .with("https://example.com/a.jsonld", ContextTail::Reference(iri("https://example.com/b.jsonld")))
            .with("https://example.com/b.jsonld", ContextTail::Reference(iri("https://example.com/a.jsonld")));

        let presented = PresentedContext::resolve(&ContextSpec::from_link(iri("https://example.com/a.jsonld")), &cyclic)
            .await
            .expect("every document loads");

        pretty_assertions::assert_eq!(
            presented.link_target(),
            ContextLinkTarget::Hosted(json!(["https://example.com/a.jsonld", core()]))
        );
    }

    /// A single inline definition names no URL, so the link is a wrapper of
    /// the definition followed by the core context.
    #[tokio::test]
    async fn a_single_inline_definition_is_linked_through_a_wrapper() {
        pretty_assertions::assert_eq!(
            target_for(json!({ "speed": "https://example.com/speed" })).await,
            ContextLinkTarget::Hosted(json!([{ "speed": "https://example.com/speed" }, core()]))
        );
    }

    /// A definition that imported the core and set only `@version` defined
    /// nothing of its own, so the request is answered under the core context
    /// alone, as one string (review test A11).
    #[tokio::test]
    async fn a_definition_holding_only_a_core_import_and_version_is_answered_under_the_core_alone() {
        let presented = presented(&ContextSpec::from_body(
            json!({ "@import": NGSI_LD_CORE_CONTEXT_V18_URL.as_str(), "@version": 1.1 }),
        ))
        .await;

        pretty_assertions::assert_eq!(presented.body(), &core());
        pretty_assertions::assert_eq!(presented.link_target(), reference(NGSI_LD_CORE_CONTEXT_URL.as_str()));
    }

    #[test]
    fn a_core_resource_names_the_core_context_alone() {
        pretty_assertions::assert_eq!(PresentedContext::core().body(), &core());
        pretty_assertions::assert_eq!(PresentedContext::core().link_target(), reference(NGSI_LD_CORE_CONTEXT_URL.as_str()));
    }

    /// Clauses 5.8.1.4 and 5.2.3: the notification context is the
    /// subscription's `jsonldContext`, followed by the core context unless it
    /// already ends with it; a core `jsonldContext` is named as written.
    #[tokio::test]
    async fn a_notification_names_its_jsonld_context_and_the_core_context_after_it_when_owed() {
        let lookup = documents();

        pretty_assertions::assert_eq!(notification_body_context(None, &lookup).await.expect("no load"), core());
        pretty_assertions::assert_eq!(notification_body_context(Some(&iri(COMPOUND)), &lookup).await.expect("loads"), json!(COMPOUND));
        pretty_assertions::assert_eq!(
            notification_body_context(Some(&iri(USER)), &lookup).await.expect("loads"),
            json!([USER, core()])
        );
        pretty_assertions::assert_eq!(
            notification_body_context(Some(&iri(NGSI_LD_CORE_CONTEXT_V18_URL.as_str())), &lookup)
                .await
                .expect("no load"),
            json!(NGSI_LD_CORE_CONTEXT_V18_URL.as_str())
        );
    }
}
