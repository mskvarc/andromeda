//! A typed operation sent with query parameters or body members its models cannot write set as written: a `q`, a
//! `csf` or an `orderBy` naming a prefixed name or an absolute IRI, which the client's query and ordering models spell
//! only as an unquoted `AttrName`.
//!
//! Everything else the request carries is the operation's own, so it differs from a typed call only in what is set,
//! and a success answer is read through the operation and held to the same model as a typed call.

use crate::{broker::BrokerClient, unchecked_outcome::read_as};
use ngsi_ld::{
    client::{ClientError, ReqwestError},
    http::{NotAnObject, QueryParameter, RequestError, ResponseError, UncheckedDocument, UncheckedRequest, operation::Operation},
    model::{InvalidJson, RawJson},
};
use std::str::{self, Utf8Error};

/// Why an operation with written parameters could not be sent or answered.
#[derive(Debug, thiserror::Error)]
pub enum WrittenParametersError {
    /// The client refused to build the request or to send it.
    #[error("the request could not be built or sent")]
    Client(#[source] Box<ClientError<ReqwestError>>),
    /// A written parameter does not make a request URI.
    #[error("a written parameter does not make a request uri")]
    Request(#[from] RequestError),
    /// The operation's body is no UTF-8 text.
    #[error("the operation's body is no text")]
    BodyText(#[from] Utf8Error),
    /// The operation's body is no JSON text.
    #[error("the operation's body is no json")]
    BodyJson(#[from] InvalidJson),
    /// The operation's body is no JSON object, so it has no member to set.
    #[error("the operation's body is no json object")]
    BodyObject(#[from] NotAnObject),
    /// The answer is an error or a success the operation does not define.
    #[error("the answer does not read through the operation")]
    Response(#[from] ResponseError),
}

/// The client's error, boxed, since it is too large to hold in the error unboxed.
fn client_error(error: ClientError<ReqwestError>) -> WrittenParametersError {
    WrittenParametersError::Client(Box::new(error))
}

/// The request `operation` sends to the broker `client` speaks to, with each parameter of `written` set to its text,
/// to send with `send_unchecked` when the answer is a refusal to match.
///
/// # Errors
///
/// Returns [`WrittenParametersError`] when the client refuses the operation or a parameter makes no request URI.
pub fn written_request<O: Operation>(
    client: &BrokerClient,
    operation: &O,
    written: &[(QueryParameter, &str)],
) -> Result<UncheckedRequest, WrittenParametersError> {
    written
        .iter()
        .try_fold(client.unchecked(operation).map_err(client_error)?, |request, (parameter, text)| {
            Ok(request.query_parameter(*parameter, text)?)
        })
}

/// Sends `operation` with each parameter of `written` set to its text, and reads the answer through the operation.
///
/// # Errors
///
/// Returns [`WrittenParametersError`] when the request cannot be built or sent, or the answer is not a success the
/// operation reads.
pub async fn send_written<O: Operation + Sync>(
    client: &BrokerClient,
    operation: &O,
    written: &[(QueryParameter, &str)],
) -> Result<O::Outcome, WrittenParametersError> {
    let request = written_request(client, operation, written)?;
    send_read(client, operation, request).await
}

/// The request `operation` sends to the broker `client` speaks to, with each member of its body named in `written`
/// set to its text, such as the `q` of a POST query, to send with `send_unchecked` when the answer is a refusal to
/// match.
///
/// # Errors
///
/// Returns [`WrittenParametersError`] when the client refuses the operation or its body is no JSON object.
pub fn written_body_request<O: Operation>(client: &BrokerClient, operation: &O, written: &[(&str, &str)]) -> Result<UncheckedRequest, WrittenParametersError> {
    let request = client.unchecked(operation).map_err(client_error)?;
    let body = str::from_utf8(request.as_request().body())?.parse::<RawJson>()?;
    let document = written
        .iter()
        .fold(UncheckedDocument::try_from(body)?, |document, (member, text)| document.set_text(member, text));
    Ok(request.document(&document))
}

/// Sends `operation` with each member of its body named in `written` set to its text, and reads the answer through
/// the operation.
///
/// # Errors
///
/// Returns [`WrittenParametersError`] when the request cannot be built or sent, or the answer is not a success the
/// operation reads.
pub async fn send_written_body<O: Operation + Sync>(
    client: &BrokerClient,
    operation: &O,
    written: &[(&str, &str)],
) -> Result<O::Outcome, WrittenParametersError> {
    let request = written_body_request(client, operation, written)?;
    send_read(client, operation, request).await
}

/// Sends `request`, started from `operation`, and reads the answer through the operation.
async fn send_read<O: Operation + Sync>(client: &BrokerClient, operation: &O, request: UncheckedRequest) -> Result<O::Outcome, WrittenParametersError> {
    let sent = request.as_request().clone();
    let response = client.send_unchecked(request).await.map_err(client_error)?;
    Ok(read_as(operation, &sent, response)?)
}

#[cfg(test)]
mod tests {
    use crate::{
        broker::client_at,
        written_parameters::{send_written, written_body_request, written_request},
    };
    use googletest::prelude::*;
    use ngsi_ld::{
        http::{
            QueryParameter,
            operation::{QueryEntities, QueryMethod},
        },
        model::Lossless,
        query::builders::SelectorTypeBuilder,
    };
    use std::str;
    use url::Url;
    use wiremock::{
        Mock,
        MockServer,
        ResponseTemplate,
        matchers::{method, query_param},
    };

    #[gtest]
    fn a_written_parameter_goes_out_beside_the_operation_parameters() -> Result<()> {
        let client = client_at(Url::parse("http://broker.test/ngsi-ld/v1/")?)?;
        let cars = SelectorTypeBuilder::from("Car").build()?;
        let query = QueryEntities::builder().entity_type(&cars).format(Lossless::Normalized).build();
        let request = written_request(
            &client,
            &query,
            &[(QueryParameter::Q, r#""ex:speed">50"#), (QueryParameter::OrderBy, r#""ex:speed""#)],
        )?;
        let uri = request.as_request().uri().to_string();
        verify_that!(
            uri,
            all!(
                contains_substring("type=Car"),
                contains_substring("q=%22ex%3Aspeed%22%3E50"),
                contains_substring("orderBy=")
            )
        )
    }

    #[gtest]
    fn a_written_body_member_goes_out_in_the_operation_body() -> Result<()> {
        let client = client_at(Url::parse("http://broker.test/ngsi-ld/v1/")?)?;
        let cars = SelectorTypeBuilder::from("Car").build()?;
        let query = QueryEntities::builder()
            .entity_type(&cars)
            .method(QueryMethod::Post)
            .format(Lossless::Normalized)
            .build();
        let request = written_body_request(&client, &query, &[("q", "ex:speed>50")])?;
        let body = str::from_utf8(request.as_request().body())?;
        verify_that!(
            body,
            all!(
                contains_substring(r#""type":"Query""#),
                contains_substring("Car"),
                contains_substring(r#""q":"ex:speed>50""#)
            )
        )
    }

    #[gtest]
    #[tokio::test]
    async fn a_success_answer_reads_through_the_operation() -> Result<()> {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(query_param("q", r#""urn:x:a;b"<50"#))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "application/json")
                    .set_body_string(r#"[{"id": "urn:ngsi-ld:Car:slow", "type": "Car"}]"#),
            )
            .mount(&server)
            .await;
        let client = client_at(Url::parse(&format!("{}/ngsi-ld/v1/", server.uri()))?)?;
        let cars = SelectorTypeBuilder::from("Car").build()?;
        let query = QueryEntities::builder().entity_type(&cars).format(Lossless::Normalized).build();
        let page = send_written(&client, &query, &[(QueryParameter::Q, r#""urn:x:a;b"<50"#)]).await?;
        verify_that!(page.items(), len(eq(1)))
    }
}
