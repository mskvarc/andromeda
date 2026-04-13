#![allow(clippy::expect_used, clippy::unwrap_used)]
const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld";

const ETSI_COMPOUND_CONTEXT_URL: &str =
    "https://forge.etsi.org/rep/cim/ngsi-ld-test-suite/-/raw/develop/resources/jsonld-contexts/ngsi-ld-test-suite-compound.jsonld";

const ETSI_TEST_SUITE_CONTEXT_URL: &str = "https://forge.etsi.org/rep/cim/ngsi-ld-test-suite/-/raw/develop/resources/jsonld-contexts/ngsi-ld-test-suite.jsonld";

const EGM_COMPOUND_CONTEXT_URL: &str = "https://easy-global-market.github.io/ngsild-api-data-models/ngsildTestSuite/ngsildTestSuiteV1.8-compound.jsonld";

/// Returns the context URL used for test fixtures.
pub const fn test_suite_context_url() -> &'static str {
    CORE_CONTEXT
}

/// Returns the compound context URL used by the ETSI NGSI-LD test suite.
pub const fn compound_context_url() -> &'static str {
    ETSI_COMPOUND_CONTEXT_URL
}

fn fixtures_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn load_raw(path: &str) -> String {
    let full_path = fixtures_dir().join(path);
    std::fs::read_to_string(&full_path).expect("failed to read fixture")
}

fn replace_context_urls(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(ctx) = map.get_mut("@context") {
                replace_in_context(ctx);
            }
            for v in map.values_mut() {
                replace_context_urls(v);
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr.iter_mut() {
                replace_context_urls(v);
            }
        }
        _ => {}
    }
}

fn replace_in_context(ctx: &mut serde_json::Value) {
    match ctx {
        serde_json::Value::String(s) if (s == ETSI_COMPOUND_CONTEXT_URL || s == ETSI_TEST_SUITE_CONTEXT_URL || s == EGM_COMPOUND_CONTEXT_URL) => {
            *s = CORE_CONTEXT.to_string();
        }
        serde_json::Value::Array(arr) => {
            for item in arr.iter_mut() {
                if let serde_json::Value::String(s) = item
                    && (s == ETSI_COMPOUND_CONTEXT_URL || s == ETSI_TEST_SUITE_CONTEXT_URL || s == EGM_COMPOUND_CONTEXT_URL)
                {
                    *s = CORE_CONTEXT.to_string();
                }
            }
        }
        _ => {}
    }
}

/// Load a fixture file, replacing the placeholder entity ID and the ETSI context URLs
/// with the core NGSI-LD context URL.
pub fn load_fixture(path: &str, id: &str) -> serde_json::Value {
    let contents = load_raw(path);
    let mut value: serde_json::Value = serde_json::from_str(&contents).expect("failed to parse fixture");

    if let Some(obj) = value.as_object_mut()
        && obj.contains_key("id")
    {
        obj.insert("id".to_string(), serde_json::Value::String(id.to_string()));
    }

    replace_context_urls(&mut value);

    value
}

/// Load a fixture file, replacing the placeholder entity ID but keeping the original context URLs.
pub fn load_fixture_original_context(path: &str, id: &str) -> serde_json::Value {
    let contents = load_raw(path);
    let mut value: serde_json::Value = serde_json::from_str(&contents).expect("failed to parse fixture");

    if let Some(obj) = value.as_object_mut()
        && obj.contains_key("id")
    {
        obj.insert("id".to_string(), serde_json::Value::String(id.to_string()));
    }

    value
}

/// Load a fragment fixture (no ID replacement, just context URL replacement).
pub fn load_fragment(path: &str) -> serde_json::Value {
    let contents = load_raw(path);
    let mut value: serde_json::Value = serde_json::from_str(&contents).expect("failed to parse fixture");

    replace_context_urls(&mut value);

    value
}

/// Load an expectation fixture for response comparison.
pub fn load_expectation(path: &str) -> serde_json::Value {
    let contents = load_raw(path);
    let mut value: serde_json::Value = serde_json::from_str(&contents).expect("failed to parse expectation");

    replace_context_urls(&mut value);

    value
}

/// Load a raw fixture file as a string (for tests that need the raw content,
/// like invalid JSON tests).
pub fn load_fixture_raw(path: &str) -> String {
    load_raw(path)
}
