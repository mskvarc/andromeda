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
        context_url::is_full_iri,
        core_term_semantics::{coercion_of, container_of, core_term_semantics},
    },
    error::JsonLdError,
    loader::http::HttpContextLoader,
};
use andromeda_ngsi_ld_vocabulary::COMPACT_TO_EXPANDED;
use dashmap::DashMap;
use serde_json::Value;
use std::{borrow::Cow, collections::HashMap, sync::Arc};

/// The `@vocab` the NGSI-LD core context declares.
const NGSI_LD_DEFAULT_CONTEXT_PREFIX: &str = "https://uri.etsi.org/ngsi-ld/default-context/";
/// The XSD namespace, available as `xsd:` without any context declaring it.
const XSD_PREFIX: &str = "http://www.w3.org/2001/XMLSchema#";
/// The NGSI-LD namespace, available as `ngsi-ld:`.
const NGSI_LD_PREFIX: &str = "https://uri.etsi.org/ngsi-ld/";
/// The GeoJSON vocabulary namespace, available as `geojson:`.
const GEOJSON_PREFIX: &str = "https://purl.org/geojson/vocab#";

/// Term resolution for one batch, covering the core context and the
/// request's own.
#[derive(Debug, Clone)]
pub struct BatchTermMap {
    /// Term name to resolved IRI, coercion, container and scoped-context flag.
    terms: HashMap<Box<str>, ResolvedTerm>,
    /// The active `@vocab`, which any term not otherwise defined falls back to.
    vocab: Box<str>,
    /// Prefix definitions, longest first so `exFoo:bar` beats `ex:` per
    /// JSON-LD 1.1 clause 4.4.
    prefix_table: Vec<(Box<str>, Box<str>)>,
    /// The core context URL this batch resolves versionless references to, so
    /// an entity's inline core context can be checked against it.
    core_url: Box<str>,
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
    /// A request context redefined a core term to a different IRI.
    UserOverridesCoreTerm,
    /// A term carries a scoped `@context`, which the fastpath does not unfold.
    HasScopedContexts,
    /// A request context could not be resolved, or the core context was
    /// omitted entirely.
    ContextUnavailable,
}

/// One term's resolution.
#[derive(Debug, Clone)]
struct ResolvedTerm {
    /// The IRI the term expands to.
    iri: Box<str>,
    /// The `@type` coercion declared for it, if any.
    coercion: Option<TermCoercion>,
    /// The `@container` declared for it, if any.
    container: Option<TermContainer>,
    /// Whether it carries a scoped `@context`.
    has_scoped_context: bool,
}

/// A request context, parsed down to what the fastpath can honour.
///
/// Only IRI expansions, `@vocab` and the scoped-context flag survive:
/// anything else makes the batch ineligible rather than being approximated.
#[derive(Debug, Clone)]
struct UserTermTable {
    /// The terms the context defines.
    terms: HashMap<Box<str>, ResolvedTerm>,
    /// The `@vocab` it declares, if it declares one.
    vocab: Option<Box<str>>,
    /// Whether any of its terms carries a scoped `@context`.
    has_scoped_contexts: bool,
}

/// The `@type` coercion a term declares.
///
/// The core context's fixed shapes are their own variants because they need no
/// resolution; a user-declared datatype is kept as written and resolved
/// through [`BatchTermMap::expand_iri`] at emit time, so a prefix the same
/// context declared applies to it too.
#[derive(Debug, Clone)]
pub(crate) enum TermCoercion {
    /// `@type: DateTime`: emitted as `{"@type": "<NGSI-LD DateTime IRI>", "@value": "<ISO>"}`.
    DateTime,
    /// `@type: @id`: emitted as `{"@id": "<value>"}`.
    Id,
    /// `@type: @vocab`: the value is a term, expanded against the active
    /// `@vocab` and emitted as `{"@id": "<expanded>"}`.
    Vocab,
    /// `@type: @json`: the value is kept verbatim as a JSON literal, emitted as
    /// `{"@type": "@json", "@value": <value>}`.
    Json,
    /// A user datatype: emitted as `{"@type": "<the expanded user IRI>", "@value": "<s>"}`.
    TypeIri(Box<str>),
}

/// The `@container` a term declares.
///
/// The fastpath models none of these on a metadata member — each reshapes the
/// value in a way the flat walker does not unfold — so a term carrying one is
/// refused rather than mis-expanded. The variants are named after the JSON-LD
/// keywords so the derivation reads exactly what the context wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TermContainer {
    /// `@container: @list`.
    List,
    /// `@container: @language`.
    Language,
    /// `@container: @index`.
    Index,
    /// `@container: @set`.
    Set,
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
            return Cow::Borrowed(&resolved.iri);
        }
        if let Some((prefix, suffix)) = term.split_once(':') {
            if let Some((_, expansion)) = self.prefix_table.iter().find(|(declared, _)| prefix == declared.as_ref()) {
                return Cow::Owned(format!("{expansion}{suffix}"));
            }
            if let Some(expansion) = builtin_prefix(prefix) {
                return Cow::Owned(format!("{expansion}{suffix}"));
            }
        }
        Cow::Owned(format!("{}{}", self.vocab, term))
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
    pub(crate) fn effective_core_url(&self) -> &str {
        &self.core_url
    }
}

/// The namespaces available without any context declaring them.
fn builtin_prefix(prefix: &str) -> Option<&'static str> {
    match prefix {
        "xsd" => Some(XSD_PREFIX),
        "ngsi-ld" => Some(NGSI_LD_PREFIX),
        "geojson" => Some(GEOJSON_PREFIX),
        _other => None,
    }
}

/// Parsed request contexts, kept per URL for the life of the engine.
///
/// The first batch naming a URL pays the resolution; every later one pays a
/// map read.
#[derive(Debug, Default)]
pub(crate) struct UserTermTableCache {
    /// Context URL to its parsed table.
    tables: DashMap<String, Arc<UserTermTable>>,
}

impl UserTermTableCache {
    /// The parsed table for a context URL, resolving it if this is the first
    /// time it has been seen.
    ///
    /// # Errors
    ///
    /// Returns the loader's failure when the URL does not resolve.
    async fn get_or_load(&self, loader: &HttpContextLoader, url: &str) -> Result<Arc<UserTermTable>, JsonLdError> {
        if let Some(cached) = self.tables.get(url) {
            return Ok(Arc::clone(cached.value()));
        }
        let value = loader.load_value(url).await?;
        let serde_value = jsonld_syntax::Value::into_serde_json((*value).clone()); // clone: the loader's copy stays in its cache
        let table = Arc::new(parse_user_context(&serde_value));
        self.tables.insert(url.to_owned(), Arc::clone(&table));
        Ok(table)
    }

    /// Drop the parsed table for a context URL.
    ///
    /// Called when the context behind that URL is deleted or reloaded, so the
    /// next batch that names it re-resolves it instead of serving a table parsed
    /// from a document that no longer exists (clause 5.13.3).
    pub(crate) fn remove(&self, url: &str) {
        self.tables.remove(url);
    }

    /// How many context URLs have been parsed, for the test that asserts a
    /// second batch over the same URL parses nothing.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.tables.len()
    }
}

/// Build the term table for one [`ContextSpec`].
pub(crate) async fn build_batch_term_map(spec: &ContextSpec, loader: &HttpContextLoader, cache: &UserTermTableCache) -> BatchTermMap {
    // The IRI of every core term comes from the compile-time table the vocab
    // macro derived from the core context; its coercion, container and
    // scoped-context flag come from the same context document, parsed for the
    // version this deployment pinned (clause 4.3.6.8). Both halves are read
    // from the context rather than restated, so the fastpath cannot drift from
    // what the processor does.
    let core_url = spec.effective_core_url();
    let semantics = core_term_semantics(core_url);
    let mut terms: HashMap<Box<str>, ResolvedTerm> = COMPACT_TO_EXPANDED
        .iter()
        .map(|(compact, expanded)| {
            let declared = semantics.get(*compact);
            (
                Box::<str>::from(*compact),
                ResolvedTerm {
                    iri: Box::<str>::from(*expanded),
                    coercion: declared.and_then(|term| term.coercion.clone()),
                    container: declared.and_then(|term| term.container),
                    has_scoped_context: declared.is_some_and(|term| term.has_scoped_context),
                },
            )
        })
        .collect();

    let mut vocab = Box::<str>::from(NGSI_LD_DEFAULT_CONTEXT_PREFIX);
    let mut prefix_table: Vec<(Box<str>, Box<str>)> = Vec::new();
    let mut eligibility = FastpathEligibility::Eligible;

    // A spec that omits the core context resolves nothing the seeded table
    // describes, so the fastpath cannot speak for it.
    match spec.core_policy() {
        CoreContextPolicy::Only | CoreContextPolicy::Append | CoreContextPolicy::AppendIfNoExplicit => {}
        CoreContextPolicy::Omit => eligibility = FastpathEligibility::Ineligible(IneligibilityReason::ContextUnavailable),
    }

    for url in spec.link_contexts() {
        if is_ngsi_ld_core_context_url(url) || url == ANDROMEDA_CORE_CONTEXT_URL {
            continue;
        }
        match cache.get_or_load(loader, url).await {
            Ok(table) => merge_user_table(&mut terms, &mut vocab, &mut prefix_table, &table, &mut eligibility),
            Err(_unresolved) => eligibility = FastpathEligibility::Ineligible(IneligibilityReason::ContextUnavailable),
        }
    }

    if let Some(body) = spec.body_context() {
        let table = parse_user_context(body);
        merge_user_table(&mut terms, &mut vocab, &mut prefix_table, &table, &mut eligibility);
    }

    // Longest first, then alphabetically, so the order is total and the
    // JSON-LD 1.1 clause 4.4 longest-match rule holds.
    prefix_table.sort_by(|left, right| right.0.len().cmp(&left.0.len()).then_with(|| left.0.cmp(&right.0)));

    BatchTermMap {
        terms,
        vocab,
        prefix_table,
        core_url: Box::<str>::from(core_url),
        eligibility,
    }
}

/// Parse a request-supplied `@context` value into the shape the fastpath uses.
fn parse_user_context(value: &Value) -> UserTermTable {
    let mut table = UserTermTable {
        terms: HashMap::new(),
        vocab: None,
        has_scoped_contexts: false,
    };
    parse_user_context_into(value, &mut table);
    table
}

/// Accumulate one `@context` node into a table, recursing through arrays and
/// through the wrapping `@context` key of a fetched context document.
fn parse_user_context_into(value: &Value, table: &mut UserTermTable) {
    match value {
        Value::Array(items) => {
            for item in items {
                parse_user_context_into(item, table);
            }
        }
        Value::Object(map) => {
            if let Some(inner) = map.get("@context") {
                parse_user_context_into(inner, table);
                return;
            }
            for (key, declaration) in map {
                if key == "@vocab" {
                    if let Some(vocab) = declaration.as_str() {
                        table.vocab = Some(Box::<str>::from(vocab));
                    }
                } else if !key.starts_with('@') {
                    // Every other JSON-LD keyword is ignored: the fastpath
                    // does not model it, and a term it defines resolves
                    // through the processor instead.
                    parse_term_declaration(key, declaration, table);
                }
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

/// Read one `term: declaration` pair of a `@context` object.
fn parse_term_declaration(term: &str, declaration: &Value, table: &mut UserTermTable) {
    match declaration {
        Value::String(iri) => {
            table.terms.insert(
                Box::<str>::from(term),
                ResolvedTerm {
                    iri: Box::<str>::from(iri.as_str()),
                    coercion: None,
                    container: None,
                    has_scoped_context: false,
                },
            );
        }
        Value::Object(definition) => {
            let has_scoped_context = definition.contains_key("@context");
            if has_scoped_context {
                table.has_scoped_contexts = true;
            }
            let Some(iri) = definition.get("@id").and_then(Value::as_str) else {
                return;
            };
            table.terms.insert(
                Box::<str>::from(term),
                ResolvedTerm {
                    iri: Box::<str>::from(iri),
                    coercion: declared_coercion(definition.get("@type").and_then(Value::as_str)),
                    container: container_of(definition.get("@container").and_then(Value::as_str)),
                    has_scoped_context,
                },
            );
        }
        // Null, booleans, numbers and arrays are not term declarations in the
        // JSON-LD context grammar; the processor decides what they mean.
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_) => {}
    }
}

/// The coercion a term's `@type` declaration asks for.
///
/// The four JSON-LD coercions the core context also uses are read by
/// [`coercion_of`]; anything else a request context names is an arbitrary
/// datatype IRI, kept as a [`TermCoercion::TypeIri`] and resolved at emit time.
fn declared_coercion(declared_type: Option<&str>) -> Option<TermCoercion> {
    coercion_of(declared_type).or_else(|| declared_type.map(|declared| TermCoercion::TypeIri(Box::<str>::from(declared))))
}

/// Merge one request context into the accumulating batch table.
///
/// Stops at the first thing the fastpath cannot honour: a scoped `@context`,
/// or a term whose IRI disagrees with the one already recorded. Redefining a
/// term to the *same* IRI is not a conflict and does not disable anything.
fn merge_user_table(
    terms: &mut HashMap<Box<str>, ResolvedTerm>,
    vocab: &mut Box<str>,
    prefix_table: &mut Vec<(Box<str>, Box<str>)>,
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
    if let Some(declared) = &table.vocab {
        *vocab = declared.clone(); // clone: the parsed table is cached and stays valid
    }
    for (term, resolved) in &table.terms {
        if let Some(existing) = terms.get(term.as_ref())
            && existing.iri != resolved.iri
        {
            *eligibility = FastpathEligibility::Ineligible(IneligibilityReason::UserOverridesCoreTerm);
            return;
        }
        // A term whose IRI ends in `/` or `#` is a namespace prefix by the
        // JSON-LD clause 4.4 convention, and is usable as one.
        if resolved.iri.ends_with('/') || resolved.iri.ends_with('#') {
            prefix_table.push((term.clone(), resolved.iri.clone())); // clone: the parsed table is cached and stays valid
        }
        terms.insert(term.clone(), resolved.clone()); // clone: the parsed table is cached and stays valid
    }
}

#[cfg(test)]
mod tests {
    use crate::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use serde_json::{Value, json};
    use std::{
        collections::HashMap,
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    /// An engine that resolves only what is embedded, never the network.
    fn offline_engine() -> JsonLdEngine {
        JsonLdEngine::builder().allow_remote(false).build().expect("engine builds")
    }

    /// A spec carrying one inline body context.
    fn body_spec(body: Value) -> ContextSpec {
        ContextSpec::none().with_body_context(Some(body))
    }

    /// A temporary context file and the URL it is preloaded under.
    fn preloaded_context(label: &str, payload: &str) -> (PathBuf, String) {
        let unique = SystemTime::now().duration_since(UNIX_EPOCH).expect("a clock after the epoch").as_nanos();
        let path = std::env::temp_dir().join(format!("andromeda-jsonld-{label}-{unique}.jsonld"));
        fs::write(&path, payload).expect("the temporary directory is writable");
        (path, format!("https://example.com/{label}-{unique}.jsonld"))
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
        let fast = engine
            .expand_with_handle(document.clone(), &handle) // clone: expanded again below
            .await
            .expect("the fastpath expands");
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
        preloads.insert(url.clone(), path.to_string_lossy().to_string()); // clone: the URL also builds the spec
        let engine = JsonLdEngine::builder()
            .allow_remote(false)
            .preload_contexts(preloads)
            .build()
            .expect("engine builds");

        let spec = ContextSpec::from_links(vec![url]);
        let handle = engine.batch_context_handle(&spec).await;
        assert!(handle.fast_eligible(), "a resolvable context of custom terms stays eligible");

        let document = json!({ "id": "urn:ngsi-ld:Device:gen2", "type": "X", "mySpeed": { "type": "Property", "value": 42 } });
        let fast = engine
            .expand_with_handle(document.clone(), &handle) // clone: expanded again below
            .await
            .expect("the fastpath expands");
        let slow = engine.expand(document, &spec).await.expect("the processor expands");

        assert_canonically_equal(&fast, &slow);
        let _ = fs::remove_file(path);
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
        let fast = engine
            .expand_with_handle(document.clone(), &handle) // clone: expanded again below
            .await
            .expect("the fastpath expands");
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
        let fast = engine
            .expand_with_handle(document.clone(), &handle) // clone: expanded again below
            .await
            .expect("the fallback expands");
        let slow = engine.expand(document, &spec).await.expect("the processor expands");

        assert_canonically_equal(&fast, &slow);
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
        let fast = engine
            .expand_with_handle(document.clone(), &handle) // clone: expanded again below
            .await
            .expect("the fastpath expands");
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
        preloads.insert(url.clone(), path.to_string_lossy().to_string()); // clone: the URL also builds the spec
        let engine = JsonLdEngine::builder()
            .allow_remote(false)
            .preload_contexts(preloads)
            .build()
            .expect("engine builds");
        let spec = ContextSpec::from_links(vec![url]);

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
        preloads.insert(url.clone(), path.to_string_lossy().to_string()); // clone: the URL also builds the spec
        let engine = JsonLdEngine::builder()
            .allow_remote(false)
            .preload_contexts(preloads)
            .build()
            .expect("engine builds");
        let spec = ContextSpec::from_links(vec![url.clone()]); // clone: also passed to invalidate below

        let _handle = engine.batch_context_handle(&spec).await;
        pretty_assertions::assert_eq!(engine.user_term_tables().len(), 1, "the batch parses the context");

        engine.invalidate_context(&url).await;
        pretty_assertions::assert_eq!(engine.user_term_tables().len(), 0, "invalidation drops the parsed table");

        let _ = fs::remove_file(path);
    }
}
