//! What a test's context source answers a forwarded request with (clause 6.3.3 and the answers of the operations a
//! registration covers), written from typed documents.

use http::{
    HeaderValue,
    StatusCode,
    header::{CONTENT_TYPE, LINK},
};
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::{
        BatchOperationResult,
        headers::{Link, LinkBuildError, LinkValue, RelationType},
    },
    model::{Entity, ErrorType, Input, Lossless, Output, ProblemDetails, Representation, evolution::TemporalEntity},
    registration::EntityMap,
};
use wiremock::ResponseTemplate;

/// A JSON answer of `status` holding `body`.
fn json(status: StatusCode, body: String) -> ResponseTemplate {
    ResponseTemplate::new(status.as_u16()).set_body_raw(body, "application/json")
}

/// 200 OK holding `entities` in normalized representation, as a source answers Query Entities.
#[must_use]
pub fn entities(entities: &[Entity<Output>]) -> ResponseTemplate {
    json(StatusCode::OK, entities_body(entities))
}

/// `entities` in normalized representation, as one JSON array.
fn entities_body(entities: &[Entity<Output>]) -> String {
    let items: Vec<String> = entities.iter().map(|entity| entity.render(Representation::Normalized).to_string()).collect();
    format!("[{}]", items.join(","))
}

/// 200 OK holding `entity` in normalized representation, as a source answers Retrieve Entity.
#[must_use]
pub fn entity(entity: &Entity<Output>) -> ResponseTemplate {
    json(StatusCode::OK, entity.render(Representation::Normalized).to_string())
}

/// 200 OK holding the temporal evolutions `entities` in normalized temporal representation, as a source answers Query
/// Temporal Evolution of Entities. A source's answer holds no member only a broker writes, so the evolutions are the
/// ones a client builds.
#[must_use]
pub fn temporal_entities(entities: &[TemporalEntity<Input>]) -> ResponseTemplate {
    let items: Vec<String> = entities.iter().map(|entity| entity.render(Lossless::Normalized).to_string()).collect();
    json(StatusCode::OK, format!("[{}]", items.join(",")))
}

/// 200 OK holding the temporal evolution `entity` in normalized temporal representation, as a source answers
/// Retrieve Temporal Evolution of an Entity.
#[must_use]
pub fn temporal_entity(entity: &TemporalEntity<Input>) -> ResponseTemplate {
    json(StatusCode::OK, entity.render(Lossless::Normalized).to_string())
}

/// 207 Multi-Status holding `result`, as a source answers a batch operation that succeeded only partly (clause
/// 5.2.16).
#[must_use]
pub fn batch_result(result: &BatchOperationResult) -> ResponseTemplate {
    json(StatusCode::MULTI_STATUS, result.render().to_string())
}

/// An answer of `status` holding the entity map `map`, as a source answers Create `EntityMap` (clause 6.34.3.1).
#[must_use]
pub fn entity_map(status: StatusCode, map: &EntityMap) -> ResponseTemplate {
    json(status, map.render().to_string())
}

/// An answer of `status` holding `problem`, as a source reports an error (clause 5.5.3).
#[must_use]
pub fn problem(status: StatusCode, problem: &ProblemDetails) -> ResponseTemplate {
    json(status, problem.render().to_string())
}

/// The problem a source reports with a `type`, a `title` and maybe a `status` but no `detail`: clause 5.5.3 asks
/// the detail of a broker's own problems, which the problem builder requires, while a source may leave it out.
#[must_use]
pub fn problem_without_detail(error_type: ErrorType, title: &str, status: Option<u16>) -> ProblemDetails {
    let mut problem = ProblemDetails::default();
    problem.error_type = Some(error_type);
    problem.title = Some(title.to_owned());
    problem.status = status;
    problem
}

/// An answer of `status` holding `body` as written under the media type `content_type`, as a source answers a body
/// that is no NGSI-LD document, which the broker under test must refuse to read.
#[must_use]
pub fn text(status: StatusCode, content_type: HeaderValue, body: &str) -> ResponseTemplate {
    ResponseTemplate::new(status.as_u16())
        .set_body_bytes(body.as_bytes())
        .insert_header(CONTENT_TYPE, content_type)
}

/// An answer of `status` without a body, as a source answers a write it performed (204) or an entity it does not
/// hold (404 without a problem).
#[must_use]
pub fn status(status: StatusCode) -> ResponseTemplate {
    ResponseTemplate::new(status.as_u16())
}

/// `answer` linking the next page at `next` (clause 6.3.10), a target the broker resolves against the request it sent,
/// as a source paginating by offset writes it (`</ngsi-ld/v1/entities?type=Vehicle&limit=1000&offset=1000>;
/// rel="next"`).
///
/// # Errors
///
/// Returns [`LinkBuildError`] when `next` cannot be written in a `Link` header.
pub fn linked_to_next(answer: ResponseTemplate, next: IriRefBuf) -> Result<ResponseTemplate, LinkBuildError> {
    let link = Link::try_new(LinkValue::builder(next).rel(RelationType::Next).build())?;
    Ok(answer.insert_header(LINK, link.to_string()))
}

#[cfg(test)]
mod tests {
    use crate::source_answer::{entities_body, entity_map, linked_to_next, problem_without_detail, status, temporal_entity, text};
    use googletest::prelude::*;
    use http::{HeaderValue, StatusCode};
    use iri_rs::iri;
    use ngsi_ld::{
        http::{BatchEntityError, BatchOperationResult},
        model::{Entity, EntityId, EntityTypeName, ErrorType, Property, evolution::TemporalEntity},
        registration::{EntityMap, EntitySource},
    };
    use wiremock::{Mock, MockServer, matchers::method};

    #[gtest]
    fn entities_are_answered_as_a_json_array() -> Result<()> {
        let device = Entity::output_builder().id("urn:ngsi-ld:Device:1").entity_type("Device").build()?;
        verify_that!(entities_body(&[device]), eq(r#"[{"id":"urn:ngsi-ld:Device:1","type":"Device"}]"#))
    }

    #[gtest]
    #[tokio::test(flavor = "multi_thread")]
    async fn a_temporal_entity_is_answered_in_the_normalized_temporal_representation() -> Result<()> {
        let evolution = TemporalEntity::builder()
            .id("urn:ngsi-ld:Vehicle:1".parse::<EntityId>()?)
            .entity_type("Vehicle".parse::<EntityTypeName>()?)
            .property("speed", Property::builder(42).observed_at("2020-08-01T12:00:00Z".parse()?))
            .build()?;
        let server = MockServer::start().await;
        Mock::given(method("GET")).respond_with(temporal_entity(&evolution)).mount(&server).await;
        let answer = reqwest::get(server.uri()).await?;
        expect_that!(answer.status().as_u16(), eq(200));
        verify_that!(
            answer.text().await?,
            all!(contains_substring(r#""id":"urn:ngsi-ld:Vehicle:1""#), contains_substring(r#""speed":[{"#))
        )
    }

    #[gtest]
    #[tokio::test(flavor = "multi_thread")]
    async fn an_entity_map_is_answered_with_its_status_as_rendered() -> Result<()> {
        let mut map = EntityMap::new("urn:ngsi-ld:EntityMap:1".parse()?, "2099-01-01T00:00:00Z".parse()?);
        map.entity_map.insert("urn:ngsi-ld:Device:1".parse()?, vec![EntitySource::Local]);
        let map = map;
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(entity_map(StatusCode::CREATED, &map))
            .mount(&server)
            .await;
        let answer = reqwest::get(server.uri()).await?;
        expect_that!(answer.status().as_u16(), eq(201));
        verify_that!(EntityMap::parse(&answer.text().await?)?, eq(&map))
    }

    #[gtest]
    fn a_problem_without_detail_writes_only_its_type_title_and_status() -> Result<()> {
        let problem = problem_without_detail(ErrorType::AlreadyExists, "AlreadyExists", Some(409));
        verify_that!(
            problem.render().to_string(),
            eq(r#"{"type":"https://uri.etsi.org/ngsi-ld/errors/AlreadyExists","title":"AlreadyExists","status":409}"#)
        )
    }

    #[gtest]
    fn a_batch_result_lists_its_successes_then_its_errors() -> Result<()> {
        let result = BatchOperationResult {
            success: vec![iri!("urn:ngsi-ld:Vehicle:2").into()],
            errors: vec![BatchEntityError {
                entity_id: iri!("urn:ngsi-ld:Vehicle:1").into(),
                error: problem_without_detail(ErrorType::ResourceNotFound, "ResourceNotFound", Some(404)),
                registration_id: None,
            }],
        };
        verify_that!(
            result.render().to_string(),
            eq(concat!(
                r#"{"success":["urn:ngsi-ld:Vehicle:2"],"errors":[{"entityId":"urn:ngsi-ld:Vehicle:1","error":"#,
                r#"{"type":"https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound","title":"ResourceNotFound","status":404}}]}"#
            ))
        )
    }

    #[gtest]
    #[tokio::test(flavor = "multi_thread")]
    async fn a_text_answer_carries_its_media_type_and_body_as_written() -> Result<()> {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(text(StatusCode::OK, HeaderValue::from_static("text/html"), "<html>x</html>"))
            .mount(&server)
            .await;
        let response = reqwest::get(server.uri()).await?;
        expect_that!(response.headers().get("content-type"), some(eq(&HeaderValue::from_static("text/html"))));
        verify_that!(response.text().await?, eq("<html>x</html>"))
    }

    #[gtest]
    #[tokio::test(flavor = "multi_thread")]
    async fn a_page_names_its_next_page_in_a_link_header() -> Result<()> {
        let server = MockServer::start().await;
        let page = linked_to_next(status(StatusCode::OK), "/a/ngsi-ld/v1/entities?type=Vehicle&limit=1000&offset=1000".parse()?)?;
        Mock::given(method("GET")).respond_with(page).mount(&server).await;
        let answer = reqwest::get(server.uri()).await?;
        verify_that!(
            answer.headers().get("link"),
            some(eq(&HeaderValue::from_static(
                r#"</a/ngsi-ld/v1/entities?type=Vehicle&limit=1000&offset=1000>; rel="next""#
            )))
        )
    }
}
