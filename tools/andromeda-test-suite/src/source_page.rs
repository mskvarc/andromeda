//! A context source answering a query page by page (clause 6.3.10): every page but the last links the next one by a
//! `Link` header with `rel="next"`, which a broker gathering the source's answer follows.

use http::header::LINK;
use iri_rs::IriRefBuf;
use ngsi_ld::http::headers::{Link, LinkBuildError, LinkValue, RelationType};
use wiremock::ResponseTemplate;

/// `answer` linking the next page at `next`, a target the broker resolves against the request it sent, as a source
/// paginating by offset writes it (`</ngsi-ld/v1/entities?type=Vehicle&limit=1000&offset=1000>; rel="next"`).
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
    use crate::{source_answer, source_page::linked_to_next};
    use googletest::prelude::*;
    use http::HeaderValue;
    use wiremock::{Mock, MockServer, matchers::method};

    #[gtest]
    #[tokio::test(flavor = "multi_thread")]
    async fn a_page_names_its_next_page_in_a_link_header() -> Result<()> {
        let server = MockServer::start().await;
        let page = linked_to_next(
            source_answer::status(http::StatusCode::OK),
            "/a/ngsi-ld/v1/entities?type=Vehicle&limit=1000&offset=1000".parse()?,
        )?;
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
