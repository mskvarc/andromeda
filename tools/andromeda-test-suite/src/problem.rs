//! Matchers on what a broker answers a call with: the status, the problem of an error response, its header fields and
//! whether it has a body; and the members of a reported problem a test compares on their own.
//!
//! A typed call succeeds only with a status its operation defines, so a successful outcome already states its
//! status (Create Entity's `Created` is 201). These matchers state the status of everything else: the error
//! response of a typed call and every response of an unchecked request.

use crate::answered_status::AnsweredStatus;
use googletest::{
    description::Description,
    matcher::{Matcher, MatcherBase, MatcherResult},
};
use http::{HeaderMap, HeaderName, StatusCode};
use iri_rs::{Iri, IriBuf, IriRefBuf, iri};
use ngsi_ld::{client::ClientError, model::ErrorType};
use std::{fmt::Debug, result::Result as StdResult};

/// The problem type of a request refused for its size (clause 6.3.2), which `ErrorType` names by no variant of its own.
const REQUEST_ENTITY_TOO_LARGE: Iri<&str> = iri!("https://uri.etsi.org/ngsi-ld/errors/RequestEntityTooLarge");

/// The problem type of a request refused for a condition expected to clear shortly, which `ErrorType` names by no
/// variant of its own: clause 6.3.2 admits the 503 of the HTTP binding, and Table 6.3.2-1 binds `InternalError` to 500.
const SERVICE_UNAVAILABLE: Iri<&str> = iri!("https://uri.etsi.org/ngsi-ld/errors/ServiceUnavailable");

/// What a matcher requires of the `title` of a problem (clause 5.5.3), and of its `detail` beside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleCheck {
    /// The title is not checked.
    Unchecked,
    /// The title is a string of at least one character.
    NonEmpty,
    /// The title and the detail are strings of at least one character, and they differ.
    WithDistinctDetail,
}

/// Matches an answer with a status and, optionally, a problem type and a non-empty title.
#[derive(Debug, Clone, MatcherBase)]
pub struct AnsweredMatcher {
    /// The status the answer carries.
    status: StatusCode,
    /// The problem types the error response may report, one of which it must; empty when the type is not checked.
    error_types: Vec<ErrorType>,
    /// Whether the title is checked.
    title: TitleCheck,
    /// The `detail` the problem must carry, when the test names it whole.
    detail: Option<String>,
    /// Texts the `detail` must contain.
    detail_contains: Vec<String>,
    /// Texts the body must contain, as received.
    body_contains: Vec<String>,
    /// Requirements on the answer's header fields.
    headers: Vec<HeaderCheck>,
    /// Whether the answer must have no body.
    without_body: bool,
}

/// A requirement on one header field of an answer.
#[derive(Debug, Clone)]
enum HeaderCheck {
    /// The field is present with exactly this value.
    Equals(HeaderName, String),
    /// The field is present and its value contains this text.
    Contains(HeaderName, String),
    /// The field is absent.
    Absent(HeaderName),
}

impl HeaderCheck {
    /// Whether `headers` meet the requirement.
    fn holds(&self, headers: &HeaderMap) -> bool {
        let value = |name: &HeaderName| headers.get(name).and_then(|value| value.to_str().ok());
        match self {
            Self::Equals(name, expected) => value(name) == Some(expected.as_str()),
            Self::Contains(name, part) => value(name).is_some_and(|value| value.contains(part.as_str())),
            Self::Absent(name) => !headers.contains_key(name),
        }
    }
}

/// An answer with `status`, whatever its body.
#[must_use]
pub const fn answered(status: StatusCode) -> AnsweredMatcher {
    AnsweredMatcher {
        status,
        error_types: Vec::new(),
        title: TitleCheck::Unchecked,
        detail: None,
        detail_contains: Vec::new(),
        body_contains: Vec::new(),
        headers: Vec::new(),
        without_body: false,
    }
}

/// An error answer with `status` and a problem of `error_type` (clause 6.3.3).
#[must_use]
pub fn problem(status: StatusCode, error_type: ErrorType) -> AnsweredMatcher {
    problem_among(status, [error_type])
}

/// An error answer with `status` and a problem of one of `error_types`, for a refusal the specification lets a broker
/// report under either type.
#[must_use]
pub fn problem_among(status: StatusCode, error_types: impl IntoIterator<Item = ErrorType>) -> AnsweredMatcher {
    AnsweredMatcher {
        error_types: error_types.into_iter().collect(),
        ..answered(status)
    }
}

impl AnsweredMatcher {
    /// Also requires a non-empty `title`.
    #[must_use]
    pub fn with_title(self) -> AnsweredMatcher {
        AnsweredMatcher {
            title: TitleCheck::NonEmpty,
            ..self
        }
    }

    /// Also requires a non-empty `title` and a non-empty `detail` that differs from it.
    #[must_use]
    pub fn with_distinct_detail(self) -> AnsweredMatcher {
        AnsweredMatcher {
            title: TitleCheck::WithDistinctDetail,
            ..self
        }
    }

    /// Also requires the `detail` to be `text`.
    #[must_use]
    pub fn with_detail(self, text: &str) -> AnsweredMatcher {
        AnsweredMatcher {
            detail: Some(text.to_owned()),
            ..self
        }
    }

    /// Also requires the `detail` to contain `text`.
    #[must_use]
    pub fn with_detail_containing(mut self, text: &str) -> AnsweredMatcher {
        self.detail_contains.push(text.to_owned());
        self
    }

    /// Also requires the body, as received, to contain `text`.
    #[must_use]
    pub fn with_body_containing(mut self, text: &str) -> AnsweredMatcher {
        self.body_contains.push(text.to_owned());
        self
    }

    /// Also requires the header field `name` with exactly `value`.
    #[must_use]
    pub fn with_header(mut self, name: HeaderName, value: &str) -> AnsweredMatcher {
        self.headers.push(HeaderCheck::Equals(name, value.to_owned()));
        self
    }

    /// Also requires the header field `name` with a value containing `part`.
    #[must_use]
    pub fn with_header_containing(mut self, name: HeaderName, part: &str) -> AnsweredMatcher {
        self.headers.push(HeaderCheck::Contains(name, part.to_owned()));
        self
    }

    /// Also requires the answer to have no header field `name`.
    #[must_use]
    pub fn without_header(mut self, name: HeaderName) -> AnsweredMatcher {
        self.headers.push(HeaderCheck::Absent(name));
        self
    }

    /// Also requires the answer to have no body.
    #[must_use]
    pub fn without_body(self) -> AnsweredMatcher {
        AnsweredMatcher { without_body: true, ..self }
    }

    /// Whether `answer` meets every requirement.
    fn holds<A: AnsweredStatus>(&self, answer: &A) -> bool {
        let heads = self.headers.is_empty()
            || answer
                .answered_headers()
                .is_some_and(|headers| self.headers.iter().all(|check| check.holds(headers)));
        heads && (!self.without_body || answer.answered_without_body() == Some(true)) && self.holds_problem(answer)
    }

    /// Whether the status and the problem of `answer` meet the requirements.
    fn holds_problem<A: AnsweredStatus>(&self, answer: &A) -> bool {
        let detail = answer.answered_problem().and_then(|problem| problem.detail.as_deref());
        let body = answer.answered_body().map(ToString::to_string);
        let error_type = answer.answered_problem().and_then(|problem| problem.error_type.as_ref());
        self.detail.as_deref().is_none_or(|expected| detail == Some(expected))
            && self
                .detail_contains
                .iter()
                .all(|text| detail.is_some_and(|detail| detail.contains(text.as_str())))
            && self
                .body_contains
                .iter()
                .all(|text| body.as_ref().is_some_and(|body| body.contains(text.as_str())))
            && answer.answered_status() == Some(self.status)
            && (self.error_types.is_empty() || error_type.is_some_and(|actual| self.error_types.contains(actual)))
            && match self.title {
                TitleCheck::Unchecked => true,
                TitleCheck::NonEmpty => answer
                    .answered_problem()
                    .and_then(|problem| problem.title.as_deref())
                    .is_some_and(|title| !title.is_empty()),
                TitleCheck::WithDistinctDetail => {
                    answer
                        .answered_problem()
                        .is_some_and(|problem| match (problem.title.as_deref(), problem.detail.as_deref()) {
                            (Some(title), Some(detail)) => !title.is_empty() && !detail.is_empty() && title != detail,
                            (None, _) | (_, None) => false,
                        })
                }
            }
    }
}

impl<A: AnsweredStatus + Debug> Matcher<&A> for AnsweredMatcher {
    fn matches(&self, actual: &A) -> MatcherResult {
        self.holds(actual).into()
    }

    fn describe(&self, result: MatcherResult) -> Description {
        let verb = match result {
            MatcherResult::Match => "is",
            MatcherResult::NoMatch => "isn't",
        };
        let error_type = match self.error_types.as_slice() {
            [] => String::new(),
            [error_type] => format!(" with a problem of type {error_type}"),
            error_types => format!(
                " with a problem of one of the types {}",
                error_types.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")
            ),
        };
        let title = match self.title {
            TitleCheck::Unchecked => "",
            TitleCheck::NonEmpty => " and a non-empty title",
            TitleCheck::WithDistinctDetail => " and a non-empty title and a non-empty detail that differ",
        };
        let heads = if self.headers.is_empty() {
            String::new()
        } else {
            format!(", header fields {:?}", self.headers)
        };
        let body = if self.without_body { ", and no body" } else { "" };
        format!("{verb} an answer with status {}{error_type}{title}{heads}{body}", self.status).into()
    }

    fn explain_match(&self, actual: &A) -> Description {
        let problem = actual.answered_problem();
        format!(
            "which was answered with status {:?}, problem type {:?}, title {:?}, detail {:?} and header fields {:?}",
            actual.answered_status(),
            problem.and_then(|problem| problem.error_type.as_ref()).map(ToString::to_string),
            problem.and_then(|problem| problem.title.as_deref()),
            problem.and_then(|problem| problem.detail.as_deref()),
            actual.answered_headers(),
        )
        .into()
    }
}

/// The problem type of a request refused for its size (clause 6.3.2).
#[must_use]
pub fn request_entity_too_large() -> ErrorType {
    ErrorType::from(IriRefBuf::from(IriBuf::from(REQUEST_ENTITY_TOO_LARGE)))
}

/// The problem type of a request refused for a condition expected to clear shortly (clause 6.3.2).
#[must_use]
pub fn service_unavailable() -> ErrorType {
    ErrorType::from(IriRefBuf::from(IriBuf::from(SERVICE_UNAVAILABLE)))
}

/// The `status` member of the problem a refused call reports, which the HTTP status repeats (clause 6.3.3).
#[must_use]
pub fn problem_status<V, E>(answer: &StdResult<V, ClientError<E>>) -> Option<u16> {
    answer.as_ref().err().and_then(ClientError::problem).and_then(|details| details.status)
}

/// The `instance` of the problem `answer` reports (IETF RFC 7807 section 3.1), the reference that identifies the
/// occurrence of the problem, when it reports one.
#[must_use]
pub fn instance_of<A: AnsweredStatus>(answer: &A) -> Option<&str> {
    answer.answered_problem().and_then(|problem| problem.instance.as_ref()).map(IriRefBuf::as_str)
}

#[cfg(test)]
mod tests {
    use crate::problem::{answered, instance_of, problem, problem_among, problem_status, request_entity_too_large};
    use bytes::Bytes;
    use googletest::prelude::*;
    use http::{HeaderMap, HeaderName, HeaderValue, Response, StatusCode, header::LOCATION};
    use iri_rs::IriRefBuf;
    use ngsi_ld::{
        client::ClientError,
        http::{ResponseError, ResponseHead},
        model::{ErrorType, ProblemDetails, RawJson},
    };
    use std::{io, result::Result as StdResult};

    /// The answer of an unchecked request in these tests.
    type Answer = StdResult<Response<Bytes>, ClientError<io::Error>>;

    /// An error answer of `status` with a problem of `error_type` titled `title`.
    fn refused(status: StatusCode, error_type: ErrorType, title: &str) -> Result<Answer> {
        let details = ProblemDetails::builder().error_type(error_type).title(title).detail("refused").build();
        Ok(Err(ClientError::Response(ResponseError::Problem {
            status,
            head: Box::new(ResponseHead::default()),
            details: Box::new(details),
            body: Box::new("{}".parse::<RawJson>()?),
        })))
    }

    #[gtest]
    fn the_header_fields_and_the_body_of_an_error_answer_are_checked() -> Result<()> {
        let mut headers = HeaderMap::new();
        headers.insert(HeaderName::from_static("ngsild-tenant"), HeaderValue::from_static("refused"));
        let answer: Answer = Err(ClientError::Response(ResponseError::Status {
            status: StatusCode::NOT_FOUND,
            head: Box::new(ResponseHead::new(headers)),
        }));
        let tenant = HeaderName::from_static("ngsild-tenant");
        expect_that!(
            &answer,
            answered(StatusCode::NOT_FOUND)
                .with_header(tenant.clone(), "refused")
                .without_header(LOCATION)
                .without_body()
        );
        expect_that!(&answer, answered(StatusCode::NOT_FOUND).with_header_containing(tenant.clone(), "fuse"));
        verify_that!(&answer, not(answered(StatusCode::NOT_FOUND).without_header(tenant)))
    }

    #[gtest]
    fn a_problem_has_a_body() -> Result<()> {
        let answer = refused(StatusCode::BAD_REQUEST, ErrorType::BadRequestData, "Bad request")?;
        verify_that!(&answer, not(answered(StatusCode::BAD_REQUEST).without_body()))
    }

    #[gtest]
    fn a_problem_of_the_expected_status_and_type_matches() -> Result<()> {
        let answer = refused(StatusCode::BAD_REQUEST, ErrorType::BadRequestData, "Bad request")?;
        expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
        expect_that!(&answer, answered(StatusCode::BAD_REQUEST));
        Ok(())
    }

    #[gtest]
    fn another_type_or_status_does_not_match() -> Result<()> {
        let answer = refused(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest, "Invalid")?;
        expect_that!(&answer, not(problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)));
        expect_that!(&answer, not(answered(StatusCode::NOT_FOUND)));
        Ok(())
    }

    #[gtest]
    fn a_problem_of_any_of_the_allowed_types_matches() -> Result<()> {
        let answer = refused(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest, "Invalid")?;
        expect_that!(
            &answer,
            problem_among(StatusCode::BAD_REQUEST, [ErrorType::BadRequestData, ErrorType::InvalidRequest])
        );
        verify_that!(
            &answer,
            not(problem_among(StatusCode::BAD_REQUEST, [ErrorType::BadRequestData, ErrorType::ResourceNotFound]))
        )
    }

    #[gtest]
    fn the_body_is_searched_as_received() -> Result<()> {
        let answer = refused(StatusCode::BAD_REQUEST, ErrorType::BadRequestData, "Bad request")?;
        expect_that!(&answer, answered(StatusCode::BAD_REQUEST).with_body_containing("{}"));
        verify_that!(&answer, not(answered(StatusCode::BAD_REQUEST).with_body_containing("endTimeAt")))
    }

    #[gtest]
    fn an_empty_title_fails_the_title_check() -> Result<()> {
        let answer = refused(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound, "")?;
        expect_that!(&answer, not(problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).with_title()));
        expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
        Ok(())
    }

    #[gtest]
    fn a_detail_distinct_from_the_title_passes_the_detail_check() -> Result<()> {
        let answer = refused(StatusCode::BAD_REQUEST, ErrorType::BadRequestData, "Bad request")?;
        verify_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_distinct_detail())
    }

    #[gtest]
    fn a_detail_equal_to_the_title_fails_the_detail_check() -> Result<()> {
        let answer = refused(StatusCode::BAD_REQUEST, ErrorType::BadRequestData, "refused")?;
        expect_that!(&answer, not(problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_distinct_detail()));
        expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
        Ok(())
    }

    #[gtest]
    fn a_success_of_an_unchecked_request_carries_its_status() -> Result<()> {
        let response = Response::builder().status(StatusCode::NO_CONTENT).body(Bytes::new())?;
        let answer: Answer = Ok(response);
        verify_that!(&answer, answered(StatusCode::NO_CONTENT))
    }

    #[gtest]
    fn a_detail_named_whole_must_be_the_detail() -> Result<()> {
        let answer = refused(StatusCode::BAD_REQUEST, ErrorType::BadRequestData, "Bad request")?;
        expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail("refused"));
        verify_that!(&answer, not(problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail("refuse")))
    }

    #[gtest]
    fn the_instance_of_a_problem_is_its_reference_text() -> Result<()> {
        let details = ProblemDetails::builder()
            .error_type(ErrorType::InvalidRequest)
            .title("Invalid request")
            .detail("refused")
            .instance(IriRefBuf::new("/ngsi-ld/v1/entities?type=A&q=bad((".to_owned())?)
            .build();
        let answer: Answer = Err(ClientError::Response(ResponseError::Problem {
            status: StatusCode::BAD_REQUEST,
            head: Box::new(ResponseHead::default()),
            details: Box::new(details),
            body: Box::new("{}".parse::<RawJson>()?),
        }));
        verify_that!(instance_of(&answer), some(eq("/ngsi-ld/v1/entities?type=A&q=bad((")))
    }

    #[gtest]
    fn an_answer_without_a_problem_has_no_instance_and_no_status_member() -> Result<()> {
        let answer: Answer = Ok(Response::builder().status(StatusCode::OK).body(Bytes::new())?);
        expect_that!(instance_of(&answer), none());
        verify_that!(problem_status(&answer), none())
    }

    #[gtest]
    fn the_status_member_of_a_problem_is_read() -> Result<()> {
        let details = ProblemDetails::builder()
            .error_type(request_entity_too_large())
            .title("Request entity too large")
            .detail("over 5 bytes")
            .status(413)
            .build();
        let answer: Answer = Err(ClientError::Response(ResponseError::Problem {
            status: StatusCode::PAYLOAD_TOO_LARGE,
            head: Box::new(ResponseHead::default()),
            details: Box::new(details),
            body: Box::new("{}".parse::<RawJson>()?),
        }));
        verify_that!(problem_status(&answer), some(eq(413)))
    }
}
