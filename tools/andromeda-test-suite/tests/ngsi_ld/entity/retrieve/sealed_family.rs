//! The eight attribute families an ngsildproof seals, each created as the `sealed` attribute of a Building of its
//! own, and the checks a retrieval of that Building answers: the seal pair spelled bare and the Data Integrity proof
//! as written.

use andromeda_test_suite::{
    attribute_matcher::{AttributeMatcher, any_attribute},
    broker::BrokerClient,
    namespace::Namespace,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, operation::CreateEntity},
    model::{Entity, EntityId, EntityTypeName, Lossless, RawJson},
};

/// Each attribute family and the members of its `sealed` attribute before `ngsildproof`, as JSON; `{ns}` stands for
/// the namespace.
pub(super) const SEALED_FAMILIES: [(&str, &str); 8] = [
    ("Property", r#""type": "Property", "value": 21.5"#),
    ("Relationship", r#""type": "Relationship", "object": "urn:ngsi-ld:Site:{ns}-1""#),
    (
        "GeoProperty",
        r#""type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.4, 52.5]}"#,
    ),
    ("LanguageProperty", r#""type": "LanguageProperty", "languageMap": {"en": "Door", "de": "Tuer"}"#),
    ("VocabProperty", r#""type": "VocabProperty", "vocab": "sensor""#),
    ("ListProperty", r#""type": "ListProperty", "valueList": [1, 2, 3]"#),
    ("ListRelationship", r#""type": "ListRelationship", "objectList": ["urn:ngsi-ld:Room:{ns}-1"]"#),
    ("JsonProperty", r#""type": "JsonProperty", "json": {"floors": 3}"#),
];

/// The Data Integrity proof every `sealed` attribute carries.
const PROOF: &str = r#"{"type": "DataIntegrityProof", "created": "2025-01-27T21:02:24Z", "verificationMethod": "https://example.edu/issuers/565049#z6MkwXG2WjeQnN....Hc6SaVWoT", "cryptosuite": "eddsa-rdfc-2022", "proofPurpose": "assertionMethod", "proofValue": "z3XrH3diVCqpVHXkE7WbnictqyQCkJBGTx....NRTzmuoWU1Y2FyqGfSV9eS"}"#;

/// Creates the Building `urn:ngsi-ld:Building:<ns>-<form>-<family>` whose `sealed` attribute holds `members` and an
/// ngsildproof sealing it to that Building and its type, requires 201, and answers the Building's id.
///
/// # Errors
///
/// Returns the failure of an identifier, the entity, the attribute text or the request.
pub(super) async fn create_sealed_building(
    broker: &BrokerClient,
    namespace: &Namespace,
    building_type: &EntityTypeName,
    form: &str,
    (family, members): (&str, &str),
) -> Result<EntityId> {
    let id = namespace.urn("Building", format!("{form}-{family}"))?;
    let building = Entity::builder().id(id.clone()).entity_type(building_type.clone()).build()?;
    let members = members.replace("{ns}", namespace.token());
    // The model refuses an attribute proof, so `sealed` is written as text.
    let sealed: RawJson = format!(
        r#"{{{members}, "ngsildproof": {{"type": "Property", "entityIdSealed": "urn:ngsi-ld:Building:{namespace}-{form}-{family}", "entityTypeSealed": "Building_{namespace}", "value": {PROOF}}}}}"#
    )
    .parse()?;
    let document = UncheckedDocument::from_entity(&building, Lossless::Normalized).set_member("sealed", &sealed);
    let request = broker.unchecked(&CreateEntity::builder().entity(&building).build())?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    Ok(id)
}

/// Expects `body` to spell neither `entityIdSealed` nor `entityTypeSealed` with a prefix or as an expanded IRI.
pub(super) fn expect_bare_seal_spelling(body: &[u8]) {
    for spelling in [
        "ngsi-ld:entityIdSealed",
        "ngsi-ld:entityTypeSealed",
        "https://uri.etsi.org/ngsi-ld/entityIdSealed",
        "https://uri.etsi.org/ngsi-ld/entityTypeSealed",
        "default-context/entityIdSealed",
        "default-context/entityTypeSealed",
    ] {
        expect_that!(String::from_utf8_lossy(body), not(contains_substring(spelling)));
    }
}

/// An attribute sealed to `id` and `building_type` whose proof holds every member of [`PROOF`].
pub(super) fn sealed_proof(id: EntityId, building_type: EntityTypeName) -> AttributeMatcher {
    any_attribute()
        .sealed_to(id)
        .sealed_type(building_type)
        .proof_member(["type"], "DataIntegrityProof")
        .proof_member(["created"], "2025-01-27T21:02:24Z")
        .proof_member(["verificationMethod"], "https://example.edu/issuers/565049#z6MkwXG2WjeQnN....Hc6SaVWoT")
        .proof_member(["cryptosuite"], "eddsa-rdfc-2022")
        .proof_member(["proofPurpose"], "assertionMethod")
        .proof_member(["proofValue"], "z3XrH3diVCqpVHXkE7WbnictqyQCkJBGTx....NRTzmuoWU1Y2FyqGfSV9eS")
}
