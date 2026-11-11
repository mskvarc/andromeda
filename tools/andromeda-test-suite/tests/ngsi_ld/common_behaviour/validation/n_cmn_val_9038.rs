//! An ngsildproof missing a proof member or a seal is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, written_json::json_request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::ErrorType;

/// The members ETSI TS 104 179 V1.1.1 clause 5.1.2 requires of a proof, written with the terms of the W3C Data
/// Integrity v2 context.
const MEMBERS: [(&str, &str); 6] = [
    ("type", r#""DataIntegrityProof""#),
    ("created", r#""2025-01-27T21:02:24Z""#),
    ("verificationMethod", r#""https://example.edu/issuers/565049#z6MkwXG2WjeQnN""#),
    ("cryptosuite", r#""eddsa-rdfc-2022""#),
    ("proofPurpose", r#""assertionMethod""#),
    ("proofValue", r#""z3XrH3diVCqpVHXkE7WbnictqyQCkJBGTx""#),
];

/// The proof holding every member but `omitted`, with each member in `replaced` written as the JSON given instead.
fn proof(omitted: Option<&str>, replaced: &[(&str, &str)]) -> String {
    let members = MEMBERS
        .iter()
        .filter(|(name, _)| Some(*name) != omitted)
        .map(|(name, written)| {
            let written = replaced.iter().find(|(member, _)| member == name).map_or(*written, |(_, other)| *other);
            format!(r#""{name}": {written}"#)
        })
        .collect::<Vec<_>>();
    format!("{{{}}}", members.join(", "))
}

/// Pins: Create Entity with an attribute whose ngsildproof holds a proof missing any one of its six required members,
/// or holding a second type, a `created` with an offset or with seven fractional digits, a verificationMethod that is
/// no URI, or an empty proofValue, answers 400 `BadRequestData`; so does one carrying only `entityIdSealed`, only
/// `entityTypeSealed`, or neither; the complete proof with both seals answers 201
/// Clause: 4.5.2.2, 4.6.3, 5.6.1.4
#[gtest]
#[tokio::test]
async fn n_cmn_val_9038_an_ngsildproof_missing_a_proof_member_or_a_seal_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let mut refused = MEMBERS
        .iter()
        .map(|(name, _)| (format!("no {name}"), proof(Some(name), &[]), true, true))
        .collect::<Vec<_>>();
    for (label, member, written) in [
        ("two types", "type", r#"["DataIntegrityProof", "Ed25519Signature2020"]"#),
        ("created with an offset", "created", r#""2025-01-27T23:02:24+02:00""#),
        ("created with seven fractional digits", "created", r#""2025-01-27T21:02:24.1234567Z""#),
        ("a verificationMethod that is no URI", "verificationMethod", r#""no uri at all""#),
        ("an empty proofValue", "proofValue", r#""""#),
    ] {
        refused.push((label.to_owned(), proof(None, &[(member, written)]), true, true));
    }
    refused.push(("only entityIdSealed".to_owned(), proof(None, &[]), true, false));
    refused.push(("only entityTypeSealed".to_owned(), proof(None, &[]), false, true));
    refused.push(("no seal".to_owned(), proof(None, &[]), false, false));
    for (position, (label, value, id_sealed, type_sealed)) in refused.iter().enumerate() {
        let id = namespace.urn("Building", format!("refused-{position}"))?;
        let mut seals = Vec::new();
        if *id_sealed {
            seals.push(format!(r#""entityIdSealed": "{id}""#));
        }
        if *type_sealed {
            seals.push(format!(r#""entityTypeSealed": "{building_type}""#));
        }
        seals.push(format!(r#""value": {value}"#));
        let body = format!(
            r#"{{"id": "{id}", "type": "{building_type}", "temperature": {{"type": "Property", "value": 21.5, "ngsildproof": {{"type": "Property", {}}}}}}}"#,
            seals.join(", ")
        );
        // The model refuses an ngsildproof that is no seal, so the body is sent as written.
        let answer = broker.send_unchecked(json_request_to(&broker, Method::POST, "entities", &body)?).await;
        expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData), "{label}");
    }
    let id = namespace.urn("Building", "sealed")?;
    let body = format!(
        r#"{{"id": "{id}", "type": "{building_type}", "temperature": {{"type": "Property", "value": 21.5, "ngsildproof": {{"type": "Property", "entityIdSealed": "{id}", "entityTypeSealed": "{building_type}", "value": {}}}}}}}"#,
        proof(None, &[])
    );
    // The model writes no seal as a member of an attribute, so the body is sent as written.
    let answer = broker.send_unchecked(json_request_to(&broker, Method::POST, "entities", &body)?).await?;
    expect_that!(answer.status(), eq(StatusCode::CREATED));
    Ok(())
}
