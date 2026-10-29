//! ETSI `050_02`: an Add @context request whose body is a term object with no `@context` is refused with 400
//! `BadRequestData`, and one whose body is not JSON with 400 `InvalidRequest` (clauses 5.13.2.4 and 6.3.2).

use crate::jsonld_context::stored_contexts::attribute_terms;
use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, wire};
use bytes::Bytes;
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, operation::AddContext},
    model::{APPLICATION_JSON, ErrorType},
};

/// Pins: Check that the payload include "@context"
/// Clause: 5.13.2
/// Corrected: `fix(050_02_01)`: expect `BadRequestData` for a valid-JSON body without @context (5.13.2.4)
/// Changed: Check Response Reason set to compares the HTTP/1.1 reason phrase, which Hurl cannot read; RFC 9112
/// clause 4 has a client ignore it and HTTP/2 carries none (RFC 9113 clause 8.3.2), so the port checks the status code
/// the phrase names
/// Vocabulary: none (the case is about @context storage itself and references no suite @context)
#[gtest]
#[tokio::test]
async fn e050_02_01_checking_incorrect_payload() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let terms = ["A1", "A2", "A3"];
    let document = terms.iter().fold(UncheckedDocument::empty(), |document, term| {
        document.set_text(term, &format!("urn:ngsi-ld:attributes:{}-{term}", namespace.token()))
    });
    let context = attribute_terms(&namespace, &terms)?;
    let request = broker.unchecked(&AddContext::builder().context(&context).build())?.document_as_is(&document);
    let answer = broker.send_unchecked(wire::content_type(request, &APPLICATION_JSON)?).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_distinct_detail());
    Ok(())
}

/// Pins: Check that the payload include "@context"
/// Clause: 5.13.2
/// Corrected: `fix(050_02_01)`: expect `BadRequestData` for a valid-JSON body without @context (5.13.2.4)
/// Changed: Check Response Reason set to compares the HTTP/1.1 reason phrase, which Hurl cannot read; RFC 9112
/// clause 4 has a client ignore it and HTTP/2 carries none (RFC 9113 clause 8.3.2), so the port checks the status code
/// the phrase names
/// Vocabulary: none (the case is about @context storage itself and references no suite @context)
#[gtest]
#[tokio::test]
async fn e050_02_02_checking_wrong_json() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let token = namespace.token();
    // Verbatim: the members lack the commas between them, a defect no typed document renders.
    let body = format!(
        "{{\n    \"A1\": \"urn:ngsi-ld:attributes:{token}-A1\"\n    \"A2\": \"urn:ngsi-ld:attributes:{token}-A2\"\n    \"A3\": \"urn:ngsi-ld:attributes:{token}-A3\"\n}}\n"
    );
    let context = attribute_terms(&namespace, &["A1", "A2", "A3"])?;
    let request = broker.unchecked(&AddContext::builder().context(&context).build())?.body(Bytes::from(body));
    let answer = broker.send_unchecked(wire::content_type(request, &APPLICATION_JSON)?).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_distinct_detail());
    Ok(())
}
