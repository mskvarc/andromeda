//! The request contexts every corpus document is compacted under.
//!
//! Each changes term selection or value compaction in its own way: none at
//! all, plain terms, prefixes, coercions to `@id`, a datatype and `@json`,
//! `@language`, `@list` and `@set` containers, a default language, terms that
//! claim a reserved IRI ahead of the core context's, a redefinition of a
//! protected core term, and the features the walk leaves to the processor:
//! scoped contexts, `@nest`, map containers. The ETSI test-suite context is
//! the one the vocabulary crate keeps; nothing is fetched.

use andromeda_json_literal::JsonLiteral;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_vocabulary::test_suite_context::ETSI_TEST_SUITE_COMPOUND_CONTEXT;
use serde_json::{Value, json};

/// The default-context namespace an unknown term expands into.
const DEFAULT_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/default-context/";

/// Whether the walk decides a context alone or leaves entities to the
/// processor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Decision {
    /// The walk compacts every corpus entity itself.
    Walk,
    /// The walk leaves at least one corpus entity to the processor.
    Processor,
}

/// One test context.
pub(crate) struct TestContext {
    /// What the context exercises.
    pub(crate) name: &'static str,
    /// The context.
    pub(crate) spec: ContextSpec,
    /// Who decides its compaction.
    pub(crate) decision: Decision,
}

/// A body context.
fn body(name: &'static str, decision: Decision, context: &Value) -> TestContext {
    TestContext {
        name,
        spec: ContextSpec::from_body(JsonLiteral::admit(&context.to_string()).expect("an admissible context")),
        decision,
    }
}

/// An IRI of the default context.
fn default_context(term: &str) -> String {
    format!("{DEFAULT_CONTEXT}{term}")
}

/// Every test context.
pub(crate) fn test_contexts() -> Vec<TestContext> {
    [term_contexts(), container_contexts(), claiming_contexts()].into_iter().flatten().collect()
}

/// The contexts that name terms, prefixes and coercions.
fn term_contexts() -> Vec<TestContext> {
    let etsi: Value = serde_json::from_str(ETSI_TEST_SUITE_COMPOUND_CONTEXT).expect("the ETSI compound context is JSON");
    vec![
        body("etsi-compound", Decision::Walk, &etsi.get("@context").cloned().unwrap_or(Value::Null)),
        TestContext {
            name: "none",
            spec: ContextSpec::none(),
            decision: Decision::Walk,
        },
        TestContext {
            name: "core-only",
            spec: ContextSpec::core_only(),
            decision: Decision::Walk,
        },
        body(
            "terms",
            Decision::Walk,
            &json!({
                "Vehicle": default_context("Vehicle"),
                "brandName": "http://example.org/vehicle/brandName",
                "speed": default_context("speed"),
                "isParked": {"@type": "@id", "@id": default_context("isParked")}
            }),
        ),
        body("vocab", Decision::Walk, &json!({"@vocab": "https://example.org/vocab/"})),
        body(
            "prefixes",
            Decision::Walk,
            &json!({"ex": "http://example.org/vehicle/", "def": DEFAULT_CONTEXT, "geo": "https://purl.org/geojson/vocab#"}),
        ),
        body(
            "id-coerced",
            Decision::Walk,
            &json!({
                "isParked": {"@id": default_context("isParked"), "@type": "@id"},
                "owner": {"@id": default_context("street"), "@type": "@id"},
                "ref": {"@id": default_context("ref"), "@type": "@vocab"}
            }),
        ),
        body(
            "datatype-coerced",
            Decision::Walk,
            &json!({
                "testedAt": {"@id": default_context("brandName"), "@type": "https://uri.etsi.org/ngsi-ld/DateTime"},
                "count": {"@id": default_context("count"), "@type": "http://www.w3.org/2001/XMLSchema#integer"}
            }),
        ),
        body(
            "json-coerced",
            Decision::Walk,
            &json!({
                "j": {"@id": "https://uri.etsi.org/ngsi-ld/hasJSON", "@type": "@json"},
                "config": {"@id": default_context("config"), "@type": "@json"},
                "payload": {"@id": default_context("payload"), "@type": "@json"}
            }),
        ),
    ]
}

/// The contexts that give terms containers, a default language or a base.
fn container_contexts() -> Vec<TestContext> {
    vec![
        body(
            "language-container",
            Decision::Walk,
            &json!({
                "vals": {"@id": "https://uri.etsi.org/ngsi-ld/hasValue", "@container": "@language"},
                "street": {"@id": default_context("street"), "@container": "@language"},
                "label": {"@id": default_context("label"), "@container": ["@language", "@set"]}
            }),
        ),
        body(
            "list-container",
            Decision::Walk,
            &json!({
                "seq": {"@id": default_context("seq"), "@container": "@list"},
                "readings": {"@id": default_context("readings"), "@container": "@list"}
            }),
        ),
        body(
            "set-container",
            Decision::Walk,
            &json!({
                "speed": {"@id": default_context("speed"), "@container": "@set"},
                "objs": {"@id": "https://uri.etsi.org/ngsi-ld/hasObject", "@type": "@id", "@container": "@set"},
                "tps": {"@id": "@type", "@container": "@set"}
            }),
        ),
        body("default-language", Decision::Walk, &json!({"@language": "en", "@direction": "ltr"})),
        body("base", Decision::Walk, &json!({"@base": "http://example.org/entities/"})),
    ]
}

/// The contexts that claim reserved IRIs, redefine a protected term, or need
/// the processor.
fn claiming_contexts() -> Vec<TestContext> {
    vec![
        body("value-hijack", Decision::Walk, &json!({"v": "https://uri.etsi.org/ngsi-ld/hasValue"})),
        body(
            "family-hijack",
            Decision::Walk,
            &json!({"P": "https://uri.etsi.org/ngsi-ld/Property", "G": "https://uri.etsi.org/ngsi-ld/GeoProperty"}),
        ),
        body("point-hijack", Decision::Walk, &json!({"Pt": "https://purl.org/geojson/vocab#Point"})),
        body(
            "coordinates-hijack",
            Decision::Processor,
            &json!({"c": {"@id": "https://purl.org/geojson/vocab#coordinates", "@container": "@list"}}),
        ),
        body(
            "protected-redefinition",
            Decision::Walk,
            &json!([
                {"@protected": true, "speed": "http://example.org/protected/speed"},
                {"speed": "http://example.org/redefined/speed"}
            ]),
        ),
        body(
            "type-scoped",
            Decision::Processor,
            &json!({"Vehicle": {"@id": default_context("Vehicle"), "@context": {"speed": "http://example.org/scoped/speed"}}}),
        ),
        body(
            "property-scoped",
            Decision::Processor,
            &json!({"address": {"@id": default_context("address"), "@context": {"street": "http://example.org/scoped/street"}}}),
        ),
        body(
            "nest",
            Decision::Processor,
            &json!({"n": "@nest", "speed": {"@id": default_context("speed"), "@nest": "n"}}),
        ),
        body(
            "index-container",
            Decision::Processor,
            &json!({"speed": {"@id": default_context("speed"), "@container": "@index"}}),
        ),
    ]
}
