#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]
//! Randomised consistency of discovery with what the entities hold.
//!
//! A seeded sequence of writes — creates, replacements, appends, value
//! updates, attribute and `datasetId` deletions, entity deletions, batch
//! upserts in both modes, merges that add a type, batch deletions — runs
//! against a private tenant over a small universe of ids, types, attribute
//! names, families and datasets, so the same keys are created, reshaped and
//! removed again and again. Every few steps the six discovery answers are
//! compared with an expectation computed here from the entities themselves, as
//! `GET /entities` renders them, by clause 5.7's rules: the types the entities
//! carry and how many carry each, the attributes and the families their
//! instances declare per type, the instances per attribute name. The census in
//! the tenant's database is compared with its recomputation from the search
//! rows too, and folded at random points, which must change nothing.
//!
//! The seed is printed; a failure names it so the sequence can be replayed.

use crate::{BASE_URL, client, provision_tenant};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_postgresql_context_store::entity::census::fold::fold_census;
use andromeda_test_harness::{discovery_census::census_drift, tenant_scoped_environment::TenantScopedEnvironment};
use reqwest::{Client, Method, StatusCode};
use serde_json::{Map, Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    env::var,
    time::{SystemTime, UNIX_EPOCH},
};

const IDS: usize = 24;
pub(crate) const TYPES: [&str; 4] = [
    "https://example.org/random/T1",
    "https://example.org/random/T2",
    "https://example.org/random/T3",
    "https://example.org/random/T4",
];
pub(crate) const ATTRIBUTES: [&str; 5] = [
    "https://example.org/random/a1",
    "https://example.org/random/a2",
    "https://example.org/random/a3",
    "https://example.org/random/a4",
    "https://example.org/random/a5",
];
const DATASETS: [Option<&str>; 3] = [None, Some("urn:ngsi-ld:Dataset:one"), Some("urn:ngsi-ld:Dataset:two")];
const STEPS: usize = 240;
const CHECK_EVERY: usize = 12;

/// A small deterministic generator (xorshift64*), so a failing sequence can
/// be replayed from its seed.
struct Sequence(u64);

impl Sequence {
    const fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, bound: usize) -> usize {
        usize::try_from(self.next() % u64::try_from(bound).expect("a small bound")).expect("a small index")
    }

    fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }
}

fn segment(iri: &str) -> String {
    iri.replace(':', "%3A").replace('/', "%2F").replace('#', "%23")
}

/// One instance of the given family, in the given dataset.
fn instance(family: usize, dataset: Option<&str>, value: u64) -> Value {
    let mut node = match family {
        0 => json!({"type": "Property", "value": value}),
        1 => json!({"type": "Relationship", "object": format!("urn:ngsi-ld:Random:{}", value % 7)}),
        2 => {
            json!({"type": "GeoProperty", "value": {"type": "Point", "coordinates": [14.0 + f64::from(u32::try_from(value % 10).expect("a digit")) / 10.0, 46.0]}})
        }
        3 => json!({"type": "LanguageProperty", "languageMap": {"en": format!("v{value}")}}),
        4 => json!({"type": "VocabProperty", "vocab": "https://example.org/random/term"}),
        5 => json!({"type": "JsonProperty", "json": {"n": value}}),
        6 => json!({"type": "ListProperty", "valueList": [value, value + 1]}),
        _ => json!({"type": "ListRelationship", "objectList": [{"object": format!("urn:ngsi-ld:Random:{}", value % 5)}]}),
    };
    if let (Some(dataset), Some(object)) = (dataset, node.as_object_mut()) {
        object.insert("datasetId".to_owned(), Value::String(dataset.to_owned()));
    }
    node
}

/// An attribute value of one to three instances, each in its own dataset.
fn attribute(sequence: &mut Sequence) -> Value {
    let family = sequence.below(8);
    let mut datasets: Vec<Option<&str>> = DATASETS.iter().copied().filter(|_| sequence.chance(45)).collect();
    if datasets.is_empty() {
        datasets.push(DATASETS[sequence.below(DATASETS.len())]);
    }
    let instances: Vec<Value> = datasets
        .into_iter()
        .map(|dataset| {
            // One attribute can mix families across its instances.
            let family = if sequence.chance(20) { sequence.below(8) } else { family };
            instance(family, dataset, sequence.next() % 1000)
        })
        .collect();
    if instances.len() == 1 {
        instances.into_iter().next().expect("one instance")
    } else {
        Value::Array(instances)
    }
}

fn types(sequence: &mut Sequence) -> Value {
    let first = TYPES[sequence.below(TYPES.len())];
    if sequence.chance(25) {
        let second = TYPES[sequence.below(TYPES.len())];
        if second != first {
            return json!([first, second]);
        }
    }
    Value::String(first.to_owned())
}

fn entity(sequence: &mut Sequence, id: &str) -> Value {
    let mut members = Map::new();
    members.insert("id".to_owned(), Value::String(id.to_owned()));
    members.insert("type".to_owned(), types(sequence));
    for name in ATTRIBUTES {
        if sequence.chance(55) {
            members.insert(name.to_owned(), attribute(sequence));
        }
    }
    Value::Object(members)
}

fn fragment(sequence: &mut Sequence) -> Value {
    let mut members = Map::new();
    members.insert(ATTRIBUTES[sequence.below(ATTRIBUTES.len())].to_owned(), attribute(sequence));
    Value::Object(members)
}

async fn send(c: &Client, tenant: &str, method: Method, path: &str, body: Option<&Value>) -> (StatusCode, Value) {
    let mut request = c.request(method, format!("{BASE_URL}{path}")).header("NGSILD-Tenant", tenant);
    if let Some(body) = body {
        request = request.header("Content-Type", "application/json").json(body);
    }
    let response = request.send().await.expect("request failed");
    let status = response.status();
    let body = response.json().await.unwrap_or_default();
    (status, body)
}

/// One random write; a refusal (a create of an existing id, a delete of a
/// missing one) is part of the sequence, never a failure of it.
async fn step(c: &Client, tenant: &str, sequence: &mut Sequence, ids: &[String]) -> (String, StatusCode) {
    let id = &ids[sequence.below(ids.len())];
    let (label, status) = match sequence.below(11) {
        0 | 1 => ("create", send(c, tenant, Method::POST, "/entities", Some(&entity(sequence, id))).await.0),
        2 => {
            let mut replacement = entity(sequence, id);
            if let Some(object) = replacement.as_object_mut() {
                object.remove("id");
            }
            ("replace", send(c, tenant, Method::PUT, &format!("/entities/{id}"), Some(&replacement)).await.0)
        }
        3 => (
            "append",
            send(c, tenant, Method::POST, &format!("/entities/{id}/attrs"), Some(&fragment(sequence)))
                .await
                .0,
        ),
        4 => (
            "update",
            send(c, tenant, Method::PATCH, &format!("/entities/{id}/attrs"), Some(&fragment(sequence)))
                .await
                .0,
        ),
        5 => {
            let name = ATTRIBUTES[sequence.below(ATTRIBUTES.len())];
            let query = match DATASETS[sequence.below(DATASETS.len())] {
                Some(dataset) if sequence.chance(60) => format!("?datasetId={dataset}"),
                Some(_) | None if sequence.chance(30) => "?deleteAll=true".to_owned(),
                Some(_) | None => String::new(),
            };
            (
                "delete attribute",
                send(c, tenant, Method::DELETE, &format!("/entities/{id}/attrs/{}{query}", segment(name)), None)
                    .await
                    .0,
            )
        }
        6 => ("delete", send(c, tenant, Method::DELETE, &format!("/entities/{id}"), None).await.0),
        7 => {
            let batch: Vec<Value> = (0..=sequence.below(5))
                .map(|_| {
                    let member = sequence.below(ids.len());
                    entity(sequence, &ids[member])
                })
                .collect();
            let mut unique: BTreeMap<String, Value> = BTreeMap::new();
            for member in batch {
                unique.insert(member["id"].as_str().expect("id").to_owned(), member);
            }
            let mode = if sequence.chance(50) { "replace" } else { "update" };
            let body = Value::Array(unique.into_values().collect());
            (
                "batch upsert",
                send(c, tenant, Method::POST, &format!("/entityOperations/upsert?options={mode}"), Some(&body))
                    .await
                    .0,
            )
        }
        8 => {
            let merge = json!({"type": types(sequence)});
            ("merge types", send(c, tenant, Method::PATCH, &format!("/entities/{id}"), Some(&merge)).await.0)
        }
        9 => {
            let doomed: BTreeSet<String> = (0..=sequence.below(4)).map(|_| ids[sequence.below(ids.len())].clone()).collect();
            let body = Value::Array(doomed.into_iter().map(Value::String).collect());
            ("batch delete", send(c, tenant, Method::POST, "/entityOperations/delete", Some(&body)).await.0)
        }
        _ => (
            "merge attributes",
            send(c, tenant, Method::PATCH, &format!("/entities/{id}"), Some(&fragment(sequence))).await.0,
        ),
    };
    assert!(status.as_u16() < 500, "{label} on {id} answered {status}");
    (format!("{label} {id}"), status)
}

/// What clause 5.7 must report, computed from the entities as rendered.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Expected {
    entity_counts: BTreeMap<String, u64>,
    type_families: BTreeMap<String, BTreeMap<String, BTreeSet<String>>>,
    instance_counts: BTreeMap<String, u64>,
    attribute_families: BTreeMap<String, BTreeSet<String>>,
    attribute_types: BTreeMap<String, BTreeSet<String>>,
}

fn as_list(value: &Value) -> Vec<&Value> {
    match value {
        Value::Array(items) => items.iter().collect(),
        Value::Null => Vec::new(),
        Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_) => vec![value],
    }
}

pub(crate) async fn expected(c: &Client, tenant: &str) -> Expected {
    let selection = TYPES.iter().map(|t| segment(t)).collect::<Vec<_>>().join(",");
    let (status, body) = send(c, tenant, Method::GET, &format!("/entities?type={selection}&limit=1000"), None).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK, "{body}");
    let mut expected = Expected::default();
    for entity in body.as_array().expect("an array of entities") {
        let types: Vec<String> = as_list(&entity["type"]).into_iter().filter_map(Value::as_str).map(str::to_owned).collect();
        for entity_type in &types {
            *expected.entity_counts.entry(entity_type.clone()).or_default() += 1;
            expected.type_families.entry(entity_type.clone()).or_default();
        }
        for name in ATTRIBUTES {
            for node in as_list(&entity[name]) {
                let family = node["type"].as_str().expect("an attribute instance declares its type").to_owned();
                *expected.instance_counts.entry(name.to_owned()).or_default() += 1;
                expected.attribute_families.entry(name.to_owned()).or_default().insert(family.clone());
                for entity_type in &types {
                    expected
                        .type_families
                        .entry(entity_type.clone())
                        .or_default()
                        .entry(name.to_owned())
                        .or_default()
                        .insert(family.clone());
                    expected.attribute_types.entry(name.to_owned()).or_default().insert(entity_type.clone());
                }
            }
        }
    }
    expected
}

fn set(value: &Value) -> BTreeSet<String> {
    as_list(value).into_iter().filter_map(Value::as_str).map(str::to_owned).collect()
}

/// The six discovery answers, read into the shape of [`Expected`].
pub(crate) async fn discovered(c: &Client, tenant: &str) -> Expected {
    let mut found = Expected::default();
    let (_, list) = send(c, tenant, Method::GET, "/types", None).await;
    let (_, details) = send(c, tenant, Method::GET, "/types?details=true", None).await;
    let listed = set(&list["typeList"]);
    let detailed: BTreeMap<String, BTreeSet<String>> = details
        .as_array()
        .expect("EntityType[]")
        .iter()
        .map(|entry| (entry["typeName"].as_str().expect("typeName").to_owned(), set(&entry["attributeNames"])))
        .collect();
    pretty_assertions::assert_eq!(
        listed,
        detailed.keys().cloned().collect::<BTreeSet<String>>(),
        "/types and /types?details disagree"
    );
    for entity_type in &listed {
        let (status, info) = send(c, tenant, Method::GET, &format!("/types/{}", segment(entity_type)), None).await;
        pretty_assertions::assert_eq!(status, StatusCode::OK, "{info}");
        found
            .entity_counts
            .insert(entity_type.clone(), info["entityCount"].as_u64().expect("entityCount"));
        let families: BTreeMap<String, BTreeSet<String>> = info["attributeDetails"]
            .as_array()
            .expect("attributeDetails")
            .iter()
            .map(|detail| {
                (
                    detail["attributeName"].as_str().expect("attributeName").to_owned(),
                    set(&detail["attributeTypes"]),
                )
            })
            .collect();
        pretty_assertions::assert_eq!(
            families.keys().cloned().collect::<BTreeSet<String>>(),
            detailed[entity_type],
            "/types/{entity_type} and /types?details disagree"
        );
        found.type_families.insert(entity_type.clone(), families);
    }
    let (_, attributes) = send(c, tenant, Method::GET, "/attributes", None).await;
    let (_, attribute_details) = send(c, tenant, Method::GET, "/attributes?details=true", None).await;
    let listed = set(&attributes["attributeList"]);
    for detail in attribute_details.as_array().expect("Attribute[]") {
        found
            .attribute_types
            .insert(detail["attributeName"].as_str().expect("attributeName").to_owned(), set(&detail["typeNames"]));
    }
    pretty_assertions::assert_eq!(
        listed,
        found.attribute_types.keys().cloned().collect::<BTreeSet<String>>(),
        "/attributes and ?details disagree"
    );
    for name in &listed {
        let (status, info) = send(c, tenant, Method::GET, &format!("/attributes/{}", segment(name)), None).await;
        pretty_assertions::assert_eq!(status, StatusCode::OK, "{info}");
        found
            .instance_counts
            .insert(name.clone(), info["attributeCount"].as_u64().expect("attributeCount"));
        found.attribute_families.insert(name.clone(), set(&info["attributeTypes"]));
        pretty_assertions::assert_eq!(set(&info["typeNames"]), found.attribute_types[name], "/attributes/{name} typeNames");
    }
    found
}

#[tokio::test]
async fn discovery_census_stays_exact_under_a_random_write_sequence() {
    let seed = var("CENSUS_SEED").ok().and_then(|raw| raw.parse().ok()).unwrap_or_else(|| {
        let since = SystemTime::now().duration_since(UNIX_EPOCH).expect("a clock after the epoch");
        since.as_secs() ^ u64::from(since.subsec_nanos()) | 1
    });
    println!("census random sequence seed {seed}");
    let env = TenantScopedEnvironment::from_suite("discovery").await.expect("private tenant environment");
    let c = client();
    let tenant = env.tenant();
    provision_tenant(&c, tenant).await;
    let ids: Vec<String> = (0..IDS).map(|n| env.id(&format!("random:{n}"))).collect();
    let pool = env.tenant_database().await.expect("the tenant database");
    let mut sequence = Sequence(seed);
    let mut history = Vec::new();
    for n in 1..=STEPS {
        history.push(step(&c, tenant, &mut sequence, &ids).await);
        if sequence.chance(4) {
            fold_census(&pool, &ContextTarget::Live).await.expect("a fold");
            history.push(("fold".to_owned(), StatusCode::OK));
        }
        if n % CHECK_EVERY == 0 || n == STEPS {
            let drift = census_drift(&pool, "public").await.expect("the census comparison");
            assert!(drift.is_empty(), "seed {seed}, step {n}: census drifted {drift:?}\nhistory {history:?}");
            let want = expected(&c, tenant).await;
            let got = discovered(&c, tenant).await;
            pretty_assertions::assert_eq!(got, want, "seed {seed}, step {n}; history {history:?}");
        }
    }
    pool.close().await;
    env.finish().await.expect("cleanup failed");
}
