//! The `@context` documents annex C's examples reference, hosted locally.
//!
//! Every payload in clauses C.2 to C.5 names vocabularies at
//! `http://example.org/ngsi-ld/latest/` -- `commonTerms.jsonld`,
//! `vehicle.jsonld`, `parking.jsonld` and the `aggregatedContext.jsonld`
//! wrapper the HTTP examples put in their `Link` header. None of those is
//! reachable, so the suite serves them itself and rewrites the origin back to
//! the printed one before any comparison.
//!
//! Their content is the merged `@context` clause C.2.4 prints, split back into
//! the three parts by the IRI namespace each term expands to. C.2.4 does not
//! define every term its own sibling examples use; the additions are recorded
//! in `docs/annex-c-corpus.md`.

use andromeda_test_mocks::{callback_path::CallbackPath, jsonld_context_server::JsonLdContextServer, server_config::MockServerConfig};
use axum::{body::Bytes, http::HeaderMap};
use serde_json::Value;
use std::{
    collections::HashMap,
    future,
    sync::{OnceLock, mpsc},
    thread,
};
use tokio::runtime::Builder;
use url::Url;

/// Origin prefix the annex prints for its example vocabularies.
pub const EXAMPLE_PREFIX: &str = "http://example.org/ngsi-ld/latest/";

/// The one hosted-context server the whole suite shares.
///
/// One server for the process means one origin to rewrite and one warm entry in
/// the broker's context cache, instead of a fresh ephemeral port -- and a fresh
/// cache miss -- for every example.
static SERVER: OnceLock<HostedExampleContexts> = OnceLock::new();

/// The URLs the suite's example `@context` documents are reachable at.
pub struct HostedExampleContexts {
    urls: HashMap<&'static str, Url>,
}

impl HostedExampleContexts {
    /// The hosted URL of one example vocabulary, by its printed file name.
    pub fn url(&self, document: &str) -> &Url {
        self.urls.get(document).expect("annex C example vocabulary is hosted by the suite")
    }
}

/// Start the shared context server, or return the one already running.
///
/// The listener is driven by a runtime of its own on a thread that never
/// finishes, because each test builds and drops its own runtime: a server
/// started inside one of those would stop answering the moment that test ended.
pub fn hosted_contexts() -> &'static HostedExampleContexts {
    SERVER.get_or_init(|| {
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let runtime = Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("the context server runtime must build");
            let (server, urls) = runtime.block_on(start());
            sender.send(urls).expect("the suite is waiting for the hosted context URLs");
            // The server is owned here and never shut down, so the listener
            // stays up for as long as the test process does.
            runtime.block_on(future::pending::<()>());
            drop(server);
        });
        HostedExampleContexts {
            urls: receiver.recv().expect("the context server thread publishes its URLs"),
        }
    })
}

/// Rewrite every hosted URL back to the origin the annex prints.
///
/// Applied to both sides of every comparison, so a payload that never mentions
/// the suite's own origin passes through unchanged.
pub fn canonicalize_example_urls(payload: &Value) -> Value {
    let Some(hosted) = SERVER.get() else {
        return payload.clone(); // clone: the caller owns the normalized copy.
    };
    let substitutions: Vec<(String, String)> = hosted
        .urls
        .iter()
        .map(|(document, url)| (url.to_string(), format!("{EXAMPLE_PREFIX}{document}")))
        .collect();
    substitute(payload, &substitutions)
}

/// Replace hosted URLs inside every string of a payload.
fn substitute(payload: &Value, substitutions: &[(String, String)]) -> Value {
    match payload {
        Value::String(text) => {
            let replaced = substitutions
                .iter()
                .fold(text.clone(), |current, (hosted, printed)| current.replace(hosted, printed)); // clone: the fold owns the rewritten copy.
            Value::String(replaced)
        }
        Value::Object(members) => Value::Object(
            members
                .iter()
                .map(|(key, value)| (key.clone(), substitute(value, substitutions))) // clone: the normalized copy owns its keys.
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(|item| substitute(item, substitutions)).collect()),
        other @ (Value::Null | Value::Bool(_) | Value::Number(_)) => other.clone(), // clone: the normalized copy owns its leaves.
    }
}

/// Bind the listener and publish each example vocabulary on it.
async fn start() -> (JsonLdContextServer, HashMap<&'static str, Url>) {
    let server = JsonLdContextServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
        .await
        .expect("annex C example context server must start");

    let mut urls = HashMap::new();
    for (document, body) in [
        ("commonTerms.jsonld", COMMON_TERMS),
        ("vehicle.jsonld", VEHICLE),
        ("parking.jsonld", PARKING),
        ("parking-abbreviated.jsonld", PARKING_ABBREVIATED),
        ("store-context.jsonld", STORE),
        ("place-context.jsonld", PLACE),
        ("multiAttribute.jsonld", MULTI_ATTRIBUTE),
        ("merged-context.jsonld", MERGED),
        ("tested-at-context.jsonld", TESTED_AT),
        ("schema-stand-in.jsonld", SCHEMA_STAND_IN),
        ("building-context.jsonld", BUILDING),
        ("vehicle-v1-context.jsonld", VEHICLE_V1),
        ("parking-c7.jsonld", PARKING_C7),
    ] {
        let url = publish(&server, document, body).await;
        urls.insert(document, url);
    }

    // The wrapper of clause C.5 references the three vocabularies by their
    // hosted URLs, so it can only be rendered once those are known.
    let wrapper = format!(
        "{{\"@context\":[\"{}\",\"{}\",\"{}\",\"https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld\"]}}",
        urls["commonTerms.jsonld"], urls["vehicle.jsonld"], urls["parking.jsonld"]
    );
    let url = publish(&server, "aggregatedContext.jsonld", &wrapper).await;
    urls.insert("aggregatedContext.jsonld", url);

    // Clause C.8 wraps a vocabulary and schema.org behind one URL, and includes
    // the Core @context as the clause calls good practice.
    let c8 = format!(
        "{{\"@context\":[\"{}\",\"{}\",\"https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld\"]}}",
        urls["vehicle-v1-context.jsonld"], urls["schema-stand-in.jsonld"]
    );
    let url = publish(&server, "wrapper-context.jsonld", &c8).await;
    urls.insert("wrapper-context.jsonld", url);

    // The same wrapper without the Core @context, which the clause says is not
    // necessary for NGSI-LD because the Core @context is always implicit.
    let c8_bare = format!(
        "{{\"@context\":[\"{}\",\"{}\"]}}",
        urls["vehicle-v1-context.jsonld"], urls["schema-stand-in.jsonld"]
    );
    let url = publish(&server, "wrapper-context-without-core.jsonld", &c8_bare).await;
    urls.insert("wrapper-context-without-core.jsonld", url);

    // Clause C.9's colliding @context array: schema.org first, the parking
    // vocabulary next, the Core @context last.
    let c9 = format!(
        "{{\"@context\":[\"{}\",\"{}\",{{\"Building\":\"http://example.org/building/Building\"}},\"https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld\"]}}",
        urls["schema-stand-in.jsonld"], urls["parking.jsonld"]
    );
    let url = publish(&server, "colliding-context.jsonld", &c9).await;
    urls.insert("colliding-context.jsonld", url);

    // The same array with the Core @context first instead of last, which the
    // clause says must not change which definition wins.
    let c9_first = format!(
        "{{\"@context\":[\"https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld\",\"{}\",{{\"Building\":\"http://example.org/building/Building\"}}]}}",
        urls["schema-stand-in.jsonld"]
    );
    let url = publish(&server, "colliding-context-core-first.jsonld", &c9_first).await;
    urls.insert("colliding-context-core-first.jsonld", url);

    (server, urls)
}

/// Serve one document and return the URL the broker can fetch it from.
async fn publish(server: &JsonLdContextServer, document: &str, body: &str) -> Url {
    let path = CallbackPath::parse(format!("/{document}")).expect("context route is absolute");
    server
        .serve(path, Bytes::from(body.to_owned()), HeaderMap::new())
        .await
        .expect("context route must be configured")
}

/// The `http://example.org/common/` half of the merged `@context` of C.2.4.
///
/// `operatedBy`, `Person`, `City`, `Camera` and `Company` are additions: the
/// merged listing omits them although its sibling examples use them.
const COMMON_TERMS: &str = r#"{"@context": {
    "isParked": {"@type": "@id", "@id": "http://example.org/common/isParked"},
    "isNextToBuilding": {"@type": "@id", "@id": "http://example.org/common/isNextToBuilding"},
    "operatedBy": {"@type": "@id", "@id": "http://example.org/common/operatedBy"},
    "providedBy": {"@type": "@id", "@id": "http://example.org/common/providedBy"},
    "reliability": "http://example.org/common/reliability",
    "name": "http://example.org/common/name",
    "Person": "http://example.org/common/Person",
    "City": "http://example.org/common/City",
    "Camera": "http://example.org/common/Camera",
    "Company": "http://example.org/common/Company"
}}"#;

/// The `http://example.org/vehicle/` half of the merged `@context` of C.2.4.
///
/// `route` and `marque` are additions used by C.2.2 and C.5.13; `Integer` is
/// printed by C.2.4 with an unterminated string, repaired here.
const VEHICLE: &str = r#"{"@context": {
    "Vehicle": "http://example.org/vehicle/Vehicle",
    "street": "http://example.org/vehicle/street",
    "brandName": "http://example.org/vehicle/brandName",
    "category": "http://example.org/vehicle/category",
    "tyreTreadDepths": "http://example.org/vehicle/tyreTreadDepths",
    "passengers": "http://example.org/vehicle/passengers",
    "route": "http://example.org/vehicle/route",
    "marque": "http://example.org/vehicle/marque",
    "speed": "http://example.org/vehicle/speed",
    "commercial": "http://example.org/vehicle/commercial",
    "non-commercial": "http://example.org/vehicle/non-commercial",
    "Integer": "http://www.w3.org/2001/XMLSchema#Integer"
}}"#;

/// The `http://example.org/parking/` half of the merged `@context` of C.2.4.
///
/// `ParkingSpot` and `status` are additions: clauses C.5.8, C.5.10 and C.5.11
/// print them as Fully Qualified Names precisely because no supplied `@context`
/// defines them, so they are declared here only to be able to name them.
const PARKING: &str = r#"{"@context": {
    "OffStreetParking": "http://example.org/parking/OffStreetParking",
    "availableSpotNumber": "http://example.org/parking/availableSpotNumber",
    "totalSpotNumber": "http://example.org/parking/totalSpotNumber"
}}"#;

/// The parking vocabulary exactly as clause C.7 prints its content.
///
/// It differs from the C.2 parking vocabulary in defining `name`, which is what
/// makes the clause's Fully-Qualified-Name example say
/// `http://example.org/parking/name`.
const PARKING_C7: &str = r#"{"@context": {
    "OffStreetParking": "http://example.org/parking/OffStreetParking",
    "availableSpotNumber": "http://example.org/parking/availableSpotNumber",
    "totalSpotNumber": "http://example.org/parking/totalSpotNumber",
    "name": "http://example.org/parking/name"
}}"#;

/// The type-coercing `@context` fragment clause C.6 prints for `testedAt`.
const TESTED_AT: &str = r#"{"@context": {
    "Vehicle": "http://example.org/vehicle/Vehicle",
    "testedAt": {
        "@type": "https://uri.etsi.org/ngsi-ld/DateTime",
        "@id": "http://example.org/test/testedAt"
    }
}}"#;

/// A stand-in for the schema.org `@context` clause C.9 collides with the core one.
///
/// The clause references the whole schema.org vocabulary; only its `location`
/// term and the `schema:` prefix that term is disambiguated through take part
/// in what the clause demonstrates, so those are what is hosted.
const SCHEMA_STAND_IN: &str = r#"{"@context": {
    "schema": "https://schema.org/",
    "location": "https://schema.org/location",
    "name": "https://schema.org/name"
}}"#;

/// The `Building` term clause C.9's example Entity needs beside the collision.
const BUILDING: &str = r#"{"@context": [
    {"Building": "http://example.org/building/Building"},
    "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld"
]}"#;

/// The vehicle vocabulary clause C.8 wraps together with schema.org.
const VEHICLE_V1: &str = r#"{"@context": {
    "Vehicle": "http://example.org/vehicle/Vehicle",
    "builtYear": "http://example.org/vehicle/builtYear",
    "speed": "http://example.org/vehicle/speed"
}}"#;

/// The merged `@context` clause C.2.4 prints, transcribed verbatim.
///
/// The printed listing leaves the `Integer` mapping's string unterminated;
/// that is repaired here and nothing else is changed.
const MERGED: &str = include_str!("corpus/c-2-4-merged-context.json");

/// The inline `@context` clause C.2.2's multi-attribute example prints.
///
/// The example carries it inline in the created Entity; a retrieval can only
/// name a `@context` by URI, so the same three terms are hosted here.
const MULTI_ATTRIBUTE: &str = r#"{"@context": {
    "Vehicle": "http://example.org/Vehicle",
    "speed": "http://example.org/speed",
    "source": "http://example.org/hasSource"
}}"#;

/// The abbreviated parking `@context` clause C.7 retrieves an Entity with.
const PARKING_ABBREVIATED: &str = r#"{"@context": {
    "OffP": "http://example.org/parking/OffStreetParking",
    "ava": "http://example.org/parking/availableSpotNumber",
    "total": "http://example.org/parking/totalSpotNumber"
}}"#;

/// The `@context` clause C.11's signed `Store` Entity references.
///
/// The clause names `https://uri.etsi.org/ngsi-ld/primer/store-context.jsonld`
/// without printing it; its terms are inferred from the Entity it is used by.
const STORE: &str = r#"{"@context": {
    "Store": "https://uri.etsi.org/ngsi-ld/primer/Store",
    "address": "https://schema.org/address",
    "streetAddress": "https://schema.org/streetAddress",
    "addressRegion": "https://schema.org/addressRegion",
    "addressLocality": "https://schema.org/addressLocality",
    "postalCode": "https://schema.org/postalCode"
}}"#;

/// The JSON-LD schema clause C.10 prints for its `Place` Entities.
///
/// The printed listing is missing a comma after `"Place"`, repaired here.
const PLACE: &str = r#"{"@context": {
    "example": "http://example.org/",
    "xsd": "http://www.w3.org/2001/XMLSchema#",
    "address": "example:address",
    "city": "example:city",
    "country": "example:country",
    "street": "example:street",
    "Place": "example:Place",
    "PostalAddress": "example:PostalAddress",
    "String": "xsd:String"
}}"#;
