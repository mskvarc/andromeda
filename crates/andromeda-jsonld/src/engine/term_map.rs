//! The term table one batch's `@context` resolves to.
//!
//! The JSON-LD processor resolves terms from a parsed context on every
//! document. [`BatchTermMap`] does it once for a whole batch: the core
//! context's compile-time table, merged with whatever the request's own
//! contexts define, plus the active `@vocab` and a prefix table sorted
//! longest-first as JSON-LD 1.1 clause 4.4 requires.
//!
//! It also decides whether the fastpath may run at all. Anything the merge
//! cannot honour exactly — a user context that redefines a core term to a
//! different IRI, a term carrying a scoped `@context`, a context that would
//! not load — makes the whole batch ineligible rather than producing an
//! expansion that differs from the processor's.

use crate::{
    context_spec::{ContextSpec, CoreContextPolicy},
    core_context::{ANDROMEDA_CORE_CONTEXT_URL, is_ngsi_ld_core_context_url},
    engine::{
        context_url::{is_absolute_iri, is_full_iri},
        core_term_semantics::core_term_semantics,
        term_map::{
            coercion::TermCoercion,
            user_term::{ResolvedTerm, UserTermTable, UserTermTableCache, parse_user_context},
        },
    },
    loader::http::HttpContextLoader,
};
use iri_rs::{Iri, IriBuf, iri};
use serde_json::Value;
use std::{borrow::Cow, collections::HashMap};

pub(crate) mod coercion;
pub(crate) mod user_term;
pub(crate) mod vocabulary;

use crate::engine::term_map::vocabulary::CoreVocabulary;

/// The XSD namespace, available as `xsd:` without any context declaring it.
static XSD_PREFIX: Iri<&'static str> = iri!("http://www.w3.org/2001/XMLSchema#");
/// The NGSI-LD namespace, available as `ngsi-ld:`.
static NGSI_LD_PREFIX: Iri<&'static str> = iri!("https://uri.etsi.org/ngsi-ld/");
/// The GeoJSON vocabulary namespace, available as `geojson:`.
static GEOJSON_PREFIX: Iri<&'static str> = iri!("https://purl.org/geojson/vocab#");

/// Term resolution for one batch, covering the core context and the
/// request's own.
#[derive(Debug, Clone)]
pub struct BatchTermMap {
    /// Term name to resolved IRI, coercion, container and scoped-context flag.
    terms: HashMap<Box<str>, ResolvedTerm>,
    /// The active `@vocab`, which any term not otherwise defined falls back to.
    vocab: IriBuf,
    /// Prefix definitions, longest first so `exFoo:bar` beats `ex:` per
    /// JSON-LD 1.1 clause 4.4.
    prefix_table: Vec<(Box<str>, IriBuf)>,
    /// The core context URL this batch resolves versionless references to, so
    /// an entity's inline core context can be checked against it.
    core_url: IriBuf,
    /// Whether the fastpath may run against this table.
    eligibility: FastpathEligibility,
}

/// Whether the fastpath may attempt expansion for a whole batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FastpathEligibility {
    /// It may. Per-entity rules in [`crate::engine::fast_entity`] still apply.
    Eligible,
    /// It may not: every entity in the batch takes the processor's path.
    Ineligible(IneligibilityReason),
}

/// Why a batch is not eligible for the fastpath.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IneligibilityReason {
    /// A request context redefined a core term to a different IRI, coercion or
    /// container — which the processor may even raise a protected-term error for.
    UserOverridesCoreTerm,
    /// A term carries a scoped `@context`, which the fastpath does not unfold.
    HasScopedContexts,
    /// A request context could not be resolved, or the core context was
    /// omitted entirely.
    ContextUnavailable,
    /// A request context uses a keyword or term-definition key the fastpath does
    /// not model but the processor honours (`@language`, `@base`, `@import`,
    /// `@reverse`, `@prefix`, …).
    UnsupportedContextFeature,
}

impl BatchTermMap {
    /// Whether the fastpath may run against this table.
    pub const fn is_eligible(&self) -> bool {
        matches!(self.eligibility, FastpathEligibility::Eligible)
    }

    /// Expand a term to its IRI.
    ///
    /// In order: an IRI is already one; then the merged term table; then the
    /// prefix table longest-first; then the four built-in prefixes; then the
    /// active `@vocab`.
    pub(crate) fn expand_iri<'a>(&'a self, term: &'a str) -> Cow<'a, str> {
        if is_full_iri(term) {
            return Cow::Borrowed(term);
        }
        if let Some(resolved) = self.terms.get(term) {
            return Cow::Borrowed(resolved.iri.as_str());
        }
        if let Some((prefix, suffix)) = term.split_once(':') {
            if let Some((_, expansion)) = self.prefix_table.iter().find(|(declared, _)| prefix == declared.as_ref()) {
                return Cow::Owned(format!("{}{suffix}", expansion.as_str()));
            }
            if let Some(expansion) = builtin_prefix(prefix) {
                return Cow::Owned(format!("{expansion}{suffix}"));
            }
            // No context maps this prefix, but the term carries a scheme, so it
            // is already an absolute IRI (clause 4.6.2) — kept as written rather
            // than vocab-expanded, the way the processor treats `did:example:1`.
            if is_absolute_iri(term) {
                return Cow::Borrowed(term);
            }
        }
        Cow::Owned(format!("{}{term}", self.vocab.as_str()))
    }

    /// The coercion declared for a term, if any.
    pub(crate) fn coercion(&self, term: &str) -> Option<&TermCoercion> {
        self.terms.get(term).and_then(|resolved| resolved.coercion.as_ref())
    }

    /// Whether a term carries a scoped `@context`.
    pub(crate) fn term_has_scoped_context(&self, term: &str) -> bool {
        self.terms.get(term).is_some_and(|resolved| resolved.has_scoped_context)
    }

    /// Whether a term declares a `@container` the fastpath does not model.
    ///
    /// A `@list`, `@language`, `@index` or `@set` container reshapes the value
    /// in a way the flat walker cannot unfold, so a metadata member carrying
    /// one is refused rather than mis-expanded.
    pub(crate) fn term_has_container(&self, term: &str) -> bool {
        self.terms.get(term).is_some_and(|resolved| resolved.container.is_some())
    }

    /// The core context URL this batch resolves versionless references to.
    pub(crate) fn effective_core_url(&self) -> Iri<&str> {
        self.core_url.as_ref()
    }
}

/// The namespaces available without any context declaring them.
fn builtin_prefix(prefix: &str) -> Option<&'static Iri<&'static str>> {
    match prefix {
        "xsd" => Some(&XSD_PREFIX),
        "ngsi-ld" => Some(&NGSI_LD_PREFIX),
        "geojson" => Some(&GEOJSON_PREFIX),
        _other => None,
    }
}

/// Build the term table for one [`ContextSpec`].
///
/// `default_core_url` is the released core context this engine expands a
/// versionless reference against (clause 4.4); the coercion, container and
/// scoped-context flags below are read from that version's document, so two
/// engines pinned to different versions build different tables.
pub(crate) async fn build_batch_term_map(
    spec: &ContextSpec,
    loader: &HttpContextLoader,
    cache: &UserTermTableCache,
    default_core_url: &IriBuf,
    core_vocabulary: &CoreVocabulary,
) -> BatchTermMap {
    // The IRI of every core term comes from the compile-time table the vocab
    // macro derived from the core context; its coercion, container and
    // scoped-context flag come from the same context document, parsed for the
    // version this deployment pinned (clause 4.4). Both halves are read
    // from the context rather than restated, so the fastpath cannot drift from
    // what the processor does.
    let core_url = default_core_url;
    let semantics = core_term_semantics(core_url);
    let mut terms: HashMap<Box<str>, ResolvedTerm> = core_vocabulary
        .iter()
        .map(|(compact, expanded)| {
            let declared = semantics.get(compact);
            (
                Box::<str>::from(compact),
                ResolvedTerm {
                    iri: expanded.clone(), // clone: each batch owns its resolved term table
                    coercion: declared.and_then(|term| term.coercion.clone()),
                    container: declared.and_then(|term| term.container),
                    has_scoped_context: declared.is_some_and(|term| term.has_scoped_context),
                    // A core term is never a user prefix candidate.
                    simple: false,
                },
            )
        })
        .collect();

    // The `@vocab` any undefined member falls back to. Seeded from the core
    // context and never overwritten: clause 5.5.7 gives the core `@vocab`
    // absolute precedence, so a user `@vocab` does not change it.
    let vocab = IriBuf::from(iri!("https://uri.etsi.org/ngsi-ld/default-context/"));
    let mut prefix_table: Vec<(Box<str>, IriBuf)> = Vec::new();
    let mut eligibility = FastpathEligibility::Eligible;

    // A spec that omits the core context resolves nothing the seeded table
    // describes, so the fastpath cannot speak for it.
    match spec.core_policy() {
        CoreContextPolicy::Only | CoreContextPolicy::Append | CoreContextPolicy::AppendIfNoExplicit => {}
        CoreContextPolicy::Omit => eligibility = FastpathEligibility::Ineligible(IneligibilityReason::ContextUnavailable),
    }

    for url in spec.link_contexts() {
        let Ok(url) = IriBuf::new(url.as_str().to_owned()) else {
            eligibility = FastpathEligibility::Ineligible(IneligibilityReason::ContextUnavailable);
            continue;
        };
        if is_ngsi_ld_core_context_url(url.as_ref()) || ANDROMEDA_CORE_CONTEXT_URL == url.as_str() {
            continue;
        }
        match cache.get_or_load(loader, &url).await {
            Ok(table) => merge_user_table(&mut terms, &mut prefix_table, &table, &mut eligibility),
            Err(_unresolved) => eligibility = FastpathEligibility::Ineligible(IneligibilityReason::ContextUnavailable),
        }
    }

    if let Some(body) = spec.body_context() {
        let mut body_items = Vec::new();
        collect_context_items(body, &mut body_items);
        for item in body_items {
            match item {
                Value::String(url) => {
                    let Ok(url) = IriBuf::new(url.to_owned()) else {
                        eligibility = FastpathEligibility::Ineligible(IneligibilityReason::ContextUnavailable);
                        break;
                    };
                    if is_ngsi_ld_core_context_url(url.as_ref()) || ANDROMEDA_CORE_CONTEXT_URL == url.as_str() {
                        continue;
                    }
                    match cache.get_or_load(loader, &url).await {
                        Ok(table) => merge_user_table(&mut terms, &mut prefix_table, &table, &mut eligibility),
                        Err(_unresolved) => {
                            eligibility = FastpathEligibility::Ineligible(IneligibilityReason::ContextUnavailable);
                            break;
                        }
                    }
                }
                Value::Object(_) => {
                    let table = parse_user_context(item);
                    merge_user_table(&mut terms, &mut prefix_table, &table, &mut eligibility);
                }
                Value::Null | Value::Bool(_) | Value::Number(_) => {
                    eligibility = FastpathEligibility::Ineligible(IneligibilityReason::UnsupportedContextFeature);
                    break;
                }
                Value::Array(_) => {
                    eligibility = FastpathEligibility::Ineligible(IneligibilityReason::UnsupportedContextFeature);
                    break;
                }
            }
        }
    }

    // Longest first, then alphabetically, so the order is total and the
    // JSON-LD 1.1 clause 4.4 longest-match rule holds.
    prefix_table.sort_by(|left, right| right.0.len().cmp(&left.0.len()).then_with(|| left.0.cmp(&right.0)));

    BatchTermMap {
        terms,
        vocab,
        prefix_table,
        core_url: core_url.clone(),
        eligibility,
    }
}

/// Flatten a body `@context` while preserving JSON-LD declaration order.
fn collect_context_items<'a>(value: &'a Value, items: &mut Vec<&'a Value>) {
    match value {
        Value::Array(values) => values.iter().for_each(|value| collect_context_items(value, items)),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_) => items.push(value),
    }
}

/// Merge one request context into the accumulating batch table.
///
/// Stops at the first thing the fastpath cannot honour: a scoped `@context`, a
/// context feature it does not model, or a term that redefines one already
/// recorded in a way the merge cannot honour. Redefining a term to the exact
/// same IRI, coercion and container is not a conflict and does not disable
/// anything. The `@vocab` is never touched — the core one wins (clause 5.5.7).
fn merge_user_table(
    terms: &mut HashMap<Box<str>, ResolvedTerm>,
    prefix_table: &mut Vec<(Box<str>, IriBuf)>,
    table: &UserTermTable,
    eligibility: &mut FastpathEligibility,
) {
    if matches!(*eligibility, FastpathEligibility::Ineligible(_)) {
        return;
    }
    if table.has_scoped_contexts {
        *eligibility = FastpathEligibility::Ineligible(IneligibilityReason::HasScopedContexts);
        return;
    }
    if table.has_unsupported_feature {
        *eligibility = FastpathEligibility::Ineligible(IneligibilityReason::UnsupportedContextFeature);
        return;
    }
    for (term, resolved) in &table.terms {
        // A redefinition that changes the IRI, the coercion or the container is
        // a conflict: comparing IRIs alone let a user context drop the coercion
        // off a protected core term (e.g. redeclaring `observedAt` to the same
        // IRI without its `@type`), which the processor raises an error for.
        if let Some(existing) = terms.get(term.as_ref())
            && (existing.iri != resolved.iri || existing.coercion != resolved.coercion || existing.container != resolved.container)
        {
            *eligibility = FastpathEligibility::Ineligible(IneligibilityReason::UserOverridesCoreTerm);
            return;
        }
        // Only a *simple* `term: "iri"` definition whose IRI ends in `/` or `#`
        // is a prefix by the JSON-LD 1.1 clause 4.4 convention; an expanded
        // (object) definition would need `@prefix: true`, which the walker does
        // not model, so it is never treated as one here.
        if resolved.simple && (resolved.iri.ends_with('/') || resolved.iri.ends_with('#')) {
            prefix_table.push((term.clone(), resolved.iri.clone())); // clone: the parsed table is cached and stays valid
        }
        terms.insert(term.clone(), resolved.clone()); // clone: the parsed table is cached and stays valid
    }
}

#[cfg(test)]
mod tests {
    use crate::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use iri_rs::IriRefBuf;
    use serde_json::{Value, json};
    use std::{
        collections::HashMap,
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };
    use url::Url;

    /// An engine that resolves only what is embedded, never the network.
    fn offline_engine() -> JsonLdEngine {
        JsonLdEngine::builder().allow_remote(false).build().expect("engine builds")
    }

    /// A context URL as the [`IriRefBuf`] a `Link` header carries.
    fn iri_ref(url: &str) -> IriRefBuf {
        IriRefBuf::new(url.to_string()).expect("a valid IRI reference")
    }

    /// A spec carrying one inline body context.
    fn body_spec(body: Value) -> ContextSpec {
        ContextSpec::none().with_body_context(Some(body))
    }

    /// A temporary context file and the URL it is preloaded under.
    fn preloaded_context(label: &str, payload: &str) -> (PathBuf, Url) {
        let unique = SystemTime::now().duration_since(UNIX_EPOCH).expect("a clock after the epoch").as_nanos();
        let path = std::env::temp_dir().join(format!("andromeda-jsonld-{label}-{unique}.jsonld"));
        fs::write(&path, payload).expect("the temporary directory is writable");
        let url = Url::parse(&format!("https://example.com/{label}-{unique}.jsonld")).expect("valid context URL");
        (path, url)
    }

    /// Object member order carries no meaning in JSON-LD, so comparisons sort
    /// keys first. Deeper canonicalisation — blank-node relabelling, list
    /// multiset semantics — is not needed for anything asserted here.
    fn canonical(value: &Value) -> Value {
        match value {
            Value::Object(map) => {
                let mut members: Vec<(String, Value)> = map.iter().map(|(key, value)| (key.clone(), canonical(value))).collect();
                members.sort_by(|left, right| left.0.cmp(&right.0));
                Value::Object(members.into_iter().collect())
            }
            Value::Array(items) => Value::Array(items.iter().map(canonical).collect()),
            other => other.clone(),
        }
    }

    /// Assert two expansions agree up to object member order.
    fn assert_canonically_equal(left: &Value, right: &Value) {
        pretty_assertions::assert_eq!(canonical(left), canonical(right));
    }

    #[tokio::test]
    async fn a_custom_term_in_a_body_context_reaches_the_fastpath_output() {
        let engine = offline_engine();
        let spec = body_spec(json!({ "mySpeed": "http://ex.com/speed" }));
        let handle = engine.batch_context_handle(&spec).await;
        assert!(handle.fast_eligible(), "a custom term is not a redefinition");

        let document = json!({ "id": "urn:ngsi-ld:Device:gen1", "type": "X", "mySpeed": { "type": "Property", "value": 42 } });
        let fast = engine.try_fast_expand(&document, &handle).expect("the fastpath expands");
        let slow = engine.expand(document, &spec).await.expect("the processor expands");

        assert!(
            fast[0].as_object().expect("an entity object").contains_key("http://ex.com/speed"),
            "the user IRI must appear in the fastpath output"
        );
        assert_canonically_equal(&fast, &slow);
    }

    #[tokio::test]
    async fn a_custom_term_in_a_context_named_by_url_reaches_the_fastpath_output() {
        let (path, url) = preloaded_context("user-context", r#"{"@context":{"mySpeed":"http://ex.com/speed"}}"#);
        let mut preloads = HashMap::new();
        preloads.insert(url.clone(), path.clone()); // clone: the URL also builds the spec
        let engine = JsonLdEngine::builder()
            .allow_remote(false)
            .preload_contexts(preloads)
            .build()
            .expect("engine builds");

        let spec = ContextSpec::from_links(vec![iri_ref(url.as_str())]);
        let handle = engine.batch_context_handle(&spec).await;
        assert!(handle.fast_eligible(), "a resolvable context of custom terms stays eligible");

        let document = json!({ "id": "urn:ngsi-ld:Device:gen2", "type": "X", "mySpeed": { "type": "Property", "value": 42 } });
        let fast = engine.try_fast_expand(&document, &handle).expect("the fastpath expands");
        let slow = engine.expand(document, &spec).await.expect("the processor expands");

        assert_canonically_equal(&fast, &slow);
        fs::remove_file(path).expect("the temporary context is removable");
    }

    #[tokio::test]
    async fn a_context_document_importing_another_context_uses_the_processor_path() {
        let (nested_path, nested_url) = preloaded_context("nested-context", r#"{"@context":{"mySpeed":"http://ex.com/speed"}}"#);
        let (outer_path, outer_url) = preloaded_context("outer-context", &format!(r#"{{"@context":["{}"]}}"#, nested_url));
        let mut preloads = HashMap::new();
        preloads.insert(nested_url.clone(), nested_path.clone());
        preloads.insert(outer_url.clone(), outer_path.clone());
        let engine = JsonLdEngine::builder()
            .allow_remote(false)
            .preload_contexts(preloads)
            .build()
            .expect("engine builds");

        let spec = body_spec(Value::String(outer_url.to_string()));
        let handle = engine.batch_context_handle(&spec).await;
        assert!(!handle.fast_eligible(), "an imported context must not use an incomplete term table");

        let document = json!({ "id": "urn:ngsi-ld:Device:imported", "type": "X", "mySpeed": { "type": "Property", "value": 42 } });
        let expanded = engine.expand(document, &spec).await.expect("the processor expands imported contexts");
        assert!(expanded[0].get("http://ex.com/speed").is_some(), "the imported term must expand through the processor");

        fs::remove_file(nested_path).expect("the nested context is removable");
        fs::remove_file(outer_path).expect("the outer context is removable");
    }

    #[tokio::test]
    async fn a_custom_term_in_a_body_context_named_by_url_reaches_the_fastpath_output() {
        let (path, url) = preloaded_context("body-user-context", r#"{"@context":{"mySpeed":"http://ex.com/speed"}}"#);
        let mut preloads = HashMap::new();
        preloads.insert(url.clone(), path.clone()); // clone: the URL also builds the body context
        let engine = JsonLdEngine::builder()
            .allow_remote(false)
            .preload_contexts(preloads)
            .build()
            .expect("engine builds");

        let spec = body_spec(Value::String(url.to_string()));
        let handle = engine.batch_context_handle(&spec).await;
        assert!(handle.fast_eligible(), "a resolvable body context URL stays eligible");

        let document = json!({ "id": "urn:ngsi-ld:Device:gen-body", "type": "X", "mySpeed": { "type": "Property", "value": 42 } });
        let fast = engine.try_fast_expand(&document, &handle).expect("the fastpath expands");
        let slow = engine.expand(document, &spec).await.expect("the processor expands");

        assert_canonically_equal(&fast, &slow);
        fs::remove_file(path).expect("the temporary context is removable");
    }

    #[tokio::test]
    async fn a_user_declared_datetime_coercion_expands_identically_on_both_paths() {
        // JSON-LD declares no `xsd` prefix by default, so the context has to
        // declare it for both paths to resolve the coercion the same way.
        let engine = offline_engine();
        let spec = body_spec(json!({
            "xsd": "http://www.w3.org/2001/XMLSchema#",
            "timestamp": { "@id": "http://ex.com/ts", "@type": "xsd:dateTime" }
        }));
        let handle = engine.batch_context_handle(&spec).await;
        assert!(handle.fast_eligible());

        let document = json!({
            "id": "urn:ngsi-ld:Device:gen3",
            "type": "X",
            "ts": { "type": "Property", "value": "2025-01-01T00:00:00Z", "timestamp": "2025-01-02T00:00:00Z" }
        });
        let fast = engine.try_fast_expand(&document, &handle).expect("the fastpath expands");
        let slow = engine.expand(document, &spec).await.expect("the processor expands");

        assert_canonically_equal(&fast, &slow);
    }

    #[tokio::test]
    async fn a_context_that_redefines_a_core_term_still_expands_correctly_through_the_fallback() {
        let engine = offline_engine();
        let spec = body_spec(json!({ "observedAt": "http://ex.com/own/observedAt" }));
        let handle = engine.batch_context_handle(&spec).await;
        assert!(!handle.fast_eligible());

        let document = json!({
            "id": "urn:ngsi-ld:Device:gen4",
            "type": "X",
            "speed": { "type": "Property", "value": 42, "observedAt": "2025-01-01T00:00:00Z" }
        });
        // The fastpath refuses (the handle is ineligible above), so production
        // routes the entity to the slow pool -- the processor -- which must
        // still expand it.
        assert!(
            engine.try_fast_expand(&document, &handle).is_none(),
            "a redefined core term must keep the fastpath from expanding"
        );
        engine.expand(document, &spec).await.expect("the processor expands");
    }

    #[tokio::test]
    async fn a_term_carrying_a_scoped_context_disables_the_fastpath() {
        let spec = body_spec(json!({
            "sensor": { "@id": "http://ex.com/sensor", "@context": { "reading": "http://ex.com/reading" } }
        }));

        let handle = offline_engine().batch_context_handle(&spec).await;

        assert!(!handle.fast_eligible(), "a scoped context redefines terms below the attribute");
    }

    #[tokio::test]
    async fn the_built_in_xsd_prefix_resolves_without_any_context_declaring_it() {
        let spec = body_spec(json!({ "size": { "@id": "http://ex.com/size", "@type": "xsd:string" } }));

        let handle = offline_engine().batch_context_handle(&spec).await;

        pretty_assertions::assert_eq!(handle.term_map().expand_iri("xsd:string").as_ref(), "http://www.w3.org/2001/XMLSchema#string");
    }

    #[tokio::test]
    async fn the_longest_matching_prefix_wins() {
        let spec = body_spec(json!({ "ex": "http://ex.com/short/", "exFoo": "http://ex.com/long/" }));

        let handle = offline_engine().batch_context_handle(&spec).await;

        assert!(handle.fast_eligible());
        pretty_assertions::assert_eq!(handle.term_map().expand_iri("exFoo:bar").as_ref(), "http://ex.com/long/bar");
    }

    #[tokio::test]
    async fn a_term_declared_with_an_id_and_no_type_expands_to_that_id() {
        let engine = offline_engine();
        let spec = body_spec(json!({ "foo": { "@id": "http://ex.com/foo" } }));
        let handle = engine.batch_context_handle(&spec).await;
        assert!(handle.fast_eligible());

        let document = json!({ "id": "urn:ngsi-ld:Device:gen8", "type": "X", "foo": { "type": "Property", "value": 1 } });
        let fast = engine.try_fast_expand(&document, &handle).expect("the fastpath expands");
        let slow = engine.expand(document, &spec).await.expect("the processor expands");

        assert!(
            fast[0].as_object().expect("an entity object").contains_key("http://ex.com/foo"),
            "the declared IRI must appear in the expanded form"
        );
        assert_canonically_equal(&fast, &slow);
    }

    #[tokio::test]
    async fn a_context_url_is_parsed_once_and_reused_by_every_later_batch() {
        let (path, url) = preloaded_context("user-cache", r#"{"@context":{"foo":"http://ex.com/foo"}}"#);
        let mut preloads = HashMap::new();
        preloads.insert(url.clone(), path.clone()); // clone: the URL also builds the spec
        let engine = JsonLdEngine::builder()
            .allow_remote(false)
            .preload_contexts(preloads)
            .build()
            .expect("engine builds");
        let spec = ContextSpec::from_links(vec![iri_ref(url.as_str())]);

        let _first = engine.batch_context_handle(&spec).await;
        pretty_assertions::assert_eq!(engine.user_term_tables().len(), 1, "the first batch parses the context");
        let _second = engine.batch_context_handle(&spec).await;
        pretty_assertions::assert_eq!(engine.user_term_tables().len(), 1, "the second batch reuses it");

        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn invalidating_a_context_drops_its_parsed_term_table() {
        // A deleted or reloaded context must not keep being served from the
        // parsed term-table cache: invalidation drops the entry so the next
        // batch re-resolves it (clause 5.13.3).
        let (path, url) = preloaded_context("user-invalidate", r#"{"@context":{"foo":"http://ex.com/foo"}}"#);
        let mut preloads = HashMap::new();
        preloads.insert(url.clone(), path.clone()); // clone: the URL also builds the spec
        let engine = JsonLdEngine::builder()
            .allow_remote(false)
            .preload_contexts(preloads)
            .build()
            .expect("engine builds");
        let spec = ContextSpec::from_links(vec![iri_ref(url.as_str())]); // url is also passed to invalidate below

        let _handle = engine.batch_context_handle(&spec).await;
        pretty_assertions::assert_eq!(engine.user_term_tables().len(), 1, "the batch parses the context");

        let url_iri = iri_ref(url.as_str()).try_into_iri().expect("absolute context URL");
        engine.invalidate_context(&url_iri).await;
        pretty_assertions::assert_eq!(engine.user_term_tables().len(), 0, "invalidation drops the parsed table");

        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn a_user_vocab_does_not_override_the_core_vocab_for_an_undefined_member() {
        // Clause 5.5.7: the core `@vocab` has absolute precedence, so an
        // undefined member expands against the core default-context prefix on
        // both paths regardless of any user `@vocab`.
        let engine = offline_engine();
        let spec = body_spec(json!({ "@vocab": "http://user.example/vocab/" }));
        let handle = engine.batch_context_handle(&spec).await;
        assert!(handle.fast_eligible(), "a user @vocab is recognised and ignored, not a refusal");

        let document = json!({ "id": "urn:ngsi-ld:Device:vocab1", "type": "X", "undefinedMember": { "type": "Property", "value": 1 } });
        let fast = engine.try_fast_expand(&document, &handle).expect("the fastpath expands");
        let slow = engine.expand(document, &spec).await.expect("the processor expands");

        let keys: Vec<&String> = fast[0].as_object().expect("an entity object").keys().collect();
        assert!(
            keys.iter().any(|key| key.contains("uri.etsi.org/ngsi-ld/default-context/undefinedMember")),
            "the core @vocab must win: {keys:?}"
        );
        assert!(!keys.iter().any(|key| key.contains("user.example")), "the user @vocab must not win: {keys:?}");
        assert_canonically_equal(&fast, &slow);
    }

    #[tokio::test]
    async fn a_context_language_keyword_the_fastpath_does_not_model_falls_back_and_agrees() {
        // `@language` sets a default language the processor applies to string
        // values; the walker does not model it, so the batch must fall back —
        // and the fallback must match the processor exactly.
        let engine = offline_engine();
        let spec = body_spec(json!({ "@language": "en", "label": "http://ex.com/label" }));
        let handle = engine.batch_context_handle(&spec).await;
        assert!(!handle.fast_eligible(), "an unmodelled context keyword disables the fastpath");

        let document = json!({ "id": "urn:ngsi-ld:Device:lang1", "type": "X", "label": { "type": "Property", "value": "hi" } });
        // The unmodelled `@language` keyword makes the fastpath refuse, so the
        // entity takes the processor path, which must still expand it.
        assert!(
            engine.try_fast_expand(&document, &handle).is_none(),
            "an unmodelled context keyword must keep the fastpath from expanding"
        );
        engine.expand(document, &spec).await.expect("the processor expands");
    }

    #[tokio::test]
    async fn redeclaring_a_core_term_without_its_coercion_disables_the_fastpath() {
        // `observedAt` is a core DateTime term. Redeclaring it to the same IRI
        // but no `@type` drops the coercion; comparing IRIs alone would have kept
        // the batch eligible and expanded the value into a different shape.
        let spec = body_spec(json!({ "observedAt": { "@id": "https://uri.etsi.org/ngsi-ld/observedAt" } }));
        let handle = offline_engine().batch_context_handle(&spec).await;
        assert!(!handle.fast_eligible(), "dropping a core term's coercion is a conflict even at the same IRI");
    }

    #[tokio::test]
    async fn a_term_definition_key_the_fastpath_does_not_model_disables_it() {
        // `@reverse` is a term-definition key the walker does not model.
        let spec = body_spec(json!({ "parentOf": { "@reverse": "http://ex.com/childOf" } }));
        let handle = offline_engine().batch_context_handle(&spec).await;
        assert!(!handle.fast_eligible(), "@reverse in a term definition disables the fastpath");
    }

    #[tokio::test]
    async fn an_absolute_iri_under_an_unlisted_scheme_is_kept_verbatim_not_vocab_expanded() {
        // Clause 4.6.2: `did:example:123` has a scheme and no context maps `did`
        // as a prefix, so it is already an absolute IRI — kept as written rather
        // than expanded against the default-context @vocab.
        let handle = offline_engine().batch_context_handle(&ContextSpec::none()).await;
        pretty_assertions::assert_eq!(handle.term_map().expand_iri("did:example:123").as_ref(), "did:example:123");
    }
}
