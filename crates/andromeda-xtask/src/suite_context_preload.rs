//! The ETSI test suite's two `@context` documents, pinned to local copies so a
//! broker under test registers them offline and never fetches them from forge.
//!
//! Every suite that writes with the test-suite vocabulary references the
//! compound `@context`, which pulls in the suite `@context` by a relative
//! reference. Fetching them live makes every run depend on forge.etsi.org being
//! up: when it answers 500, nearly every write is refused with
//! `LdContextNotAvailable` and the run says nothing about the broker.

use crate::error::XtaskError;
use iri_rs::{Iri, iri};
use std::{collections::BTreeMap, path::Path};

/// The compound `@context` the suites' fixtures and queries reference.
const SUITE_COMPOUND_CONTEXT_URL: Iri<&str> =
    iri!("https://forge.etsi.org/rep/cim/ngsi-ld-test-suite/-/raw/develop/resources/jsonld-contexts/ngsi-ld-test-suite-compound.jsonld");
/// The suite `@context` the compound references relatively; the loader resolves
/// the relative reference against the compound's URL to exactly this URL.
const SUITE_CONTEXT_URL: Iri<&str> =
    iri!("https://forge.etsi.org/rep/cim/ngsi-ld-test-suite/-/raw/develop/resources/jsonld-contexts/ngsi-ld-test-suite.jsonld");
/// File name of the compound `@context` inside a context directory.
const SUITE_COMPOUND_CONTEXT_FILE: &str = "ngsi-ld-test-suite-compound.jsonld";
/// File name of the suite `@context` inside a context directory.
const SUITE_CONTEXT_FILE: &str = "ngsi-ld-test-suite.jsonld";

/// The JSON `url -> file` map for `ANDROMEDA__JSONLD__PRELOAD_CONTEXTS` that pins
/// both suite `@context` documents to their copies in `context_dir`.
///
/// `serde_json` renders the paths, so a path containing a quote or backslash is
/// escaped rather than producing malformed JSON.
///
/// # Errors
///
/// Returns [`XtaskError::ContextPreloadSerialize`] when the map cannot be
/// rendered.
pub(crate) fn preload_map(context_dir: &Path) -> Result<String, XtaskError> {
    let compound = context_dir.join(SUITE_COMPOUND_CONTEXT_FILE);
    let suite = context_dir.join(SUITE_CONTEXT_FILE);
    let map = BTreeMap::from([
        (SUITE_COMPOUND_CONTEXT_URL.as_str(), compound.to_string_lossy()),
        (SUITE_CONTEXT_URL.as_str(), suite.to_string_lossy()),
    ]);
    serde_json::to_string(&map).map_err(XtaskError::ContextPreloadSerialize)
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::suite_context_preload::preload_map;
    use std::{collections::BTreeMap, fs, path::Path};

    /// Both suite URLs map to the file of the same name in the given directory.
    #[test]
    fn both_suite_contexts_map_to_their_local_files() {
        let rendered = preload_map(Path::new("/contexts")).expect("the map renders");
        let map: BTreeMap<String, String> = serde_json::from_str(&rendered).expect("the map is JSON");
        assert_eq!(
            map.get("https://forge.etsi.org/rep/cim/ngsi-ld-test-suite/-/raw/develop/resources/jsonld-contexts/ngsi-ld-test-suite.jsonld"),
            Some(&"/contexts/ngsi-ld-test-suite.jsonld".to_owned())
        );
        assert_eq!(
            map.get("https://forge.etsi.org/rep/cim/ngsi-ld-test-suite/-/raw/develop/resources/jsonld-contexts/ngsi-ld-test-suite-compound.jsonld"),
            Some(&"/contexts/ngsi-ld-test-suite-compound.jsonld".to_owned())
        );
    }

    /// The repository copies the host-broker runner points at exist and parse.
    #[test]
    fn the_repository_copies_are_json() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/contexts");
        for file in ["ngsi-ld-test-suite.jsonld", "ngsi-ld-test-suite-compound.jsonld"] {
            let text = fs::read_to_string(dir.join(file)).expect("the copy exists");
            let _document: serde_json::Value = serde_json::from_str(&text).expect("the copy is JSON");
        }
    }
}
