//! The suite's term document (`ngsi-ld-test-suite.jsonld`), served by the case's own server for the broker to
//! download and cache, every suite term mapped to the case's namespace.

use andromeda_test_suite::{namespace::Namespace, remote_context::RemoteContexts};
use googletest::prelude::*;
use std::collections::BTreeMap;
use url::Url;

/// The path the suite serves its term document at.
const PATH: &str = "forge.etsi.org/rep/cim/ngsi-ld-test-suite/-/raw/develop/resources/jsonld-contexts/ngsi-ld-test-suite.jsonld";

/// The terms of the suite's term document.
const TERMS: [&str; 15] = [
    "Building",
    "Vehicle",
    "OffStreetParking",
    "airQualityLevel",
    "almostFull",
    "availableSpotsNumber",
    "brandName",
    "fuelLevel",
    "isParked",
    "name",
    "reliability",
    "source",
    "speed",
    "totalSpotsNumber",
    "subCategory",
];

/// Serves the term document on `remote` as `application/ld+json`, each term mapped to
/// `https://test.andromeda/<ns>/<term>`, and answers its URL.
pub(crate) async fn serve_suite_terms(remote: &RemoteContexts, namespace: &Namespace) -> Result<Url> {
    let mut terms = BTreeMap::new();
    for term in TERMS {
        terms.insert(term, namespace.vocabulary_iri(term)?.into_inner());
    }
    let terms = terms;
    let document = serde_json::to_string(&BTreeMap::from([("@context", terms)]))?;
    Ok(remote.serve(PATH, &document, Some("application/ld+json")).await?)
}
