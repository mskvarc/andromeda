//! The forwarded requests a test's context source answers or is read for, described the way a stub mapping of a
//! source does: a method, a path or a pattern the whole path matches, query parameters, header fields and `JSONPath`
//! queries on the body.

use crate::{body_pattern::BodyPattern, context_source::SourceError, header_condition::HeaderCondition};
use http::Method;
use regex::Regex;
use std::fmt;
use wiremock::{
    Mock,
    Request,
    ResponseTemplate,
    matchers::{method, path, path_regex, query_param, query_param_contains, query_param_is_missing},
};

/// The forwarded requests a source answer or a read applies to: a method or any, a path beneath the source's root or
/// a pattern of paths, the query parameters they carry, `JSONPath` queries their body must select something with, and
/// an answer's priority over the source's other answers (1 first).
#[derive(Debug, Clone)]
pub struct SourceRequest {
    /// The method, or `None` for any.
    verb: Option<Method>,
    /// The exact path, or a regular expression the whole path matches.
    path: SourcePath,
    /// What the request's query parameters hold.
    query: Vec<QueryCondition>,
    /// What the request's header fields hold.
    headers: Vec<HeaderCondition>,
    /// The conditions the request's body meets.
    body: Vec<BodyPattern>,
    /// The priority, when the answer must win over or yield to another.
    priority: Option<u8>,
}

/// A requirement on one query parameter of a [`SourceRequest`].
#[derive(Debug, Clone)]
enum QueryCondition {
    /// The parameter is the value.
    Is(String, String),
    /// The parameter holds the text.
    Contains(String, String),
    /// The parameter is absent.
    Absent(String),
}

impl QueryCondition {
    /// Whether `request` meets the condition.
    fn holds(&self, request: &Request) -> bool {
        let mut values = request.url.query_pairs();
        match self {
            QueryCondition::Is(name, value) => values.any(|(key, actual)| key == name.as_str() && actual == value.as_str()),
            QueryCondition::Contains(name, part) => values.any(|(key, actual)| key == name.as_str() && actual.contains(part.as_str())),
            QueryCondition::Absent(name) => !values.any(|(key, _)| key == name.as_str()),
        }
    }
}

/// How a [`SourceRequest`] matches the path.
#[derive(Debug, Clone)]
enum SourcePath {
    /// The path itself.
    Exact(String),
    /// A regular expression the whole path matches, as a stub's `urlPathPattern` does.
    Pattern(String),
}

impl SourcePath {
    /// The regular expression a request path matches in whole.
    fn anchored(pattern: &str) -> String {
        format!("^(?:{pattern})$")
    }
}

impl SourceRequest {
    /// The `verb` requests on `request_path`.
    #[must_use]
    pub fn new(verb: Method, request_path: &str) -> SourceRequest {
        SourceRequest {
            verb: Some(verb),
            ..SourceRequest::any(request_path)
        }
    }

    /// The requests of any method on `request_path`.
    #[must_use]
    pub fn any(request_path: &str) -> SourceRequest {
        SourceRequest {
            verb: None,
            path: SourcePath::Exact(request_path.to_owned()),
            query: Vec::new(),
            headers: Vec::new(),
            body: Vec::new(),
            priority: None,
        }
    }

    /// The `verb` requests on a path the regular expression `pattern` matches in whole.
    #[must_use]
    pub fn matching(verb: Method, pattern: &str) -> SourceRequest {
        SourceRequest {
            verb: Some(verb),
            ..SourceRequest::any_matching(pattern)
        }
    }

    /// The requests of any method on a path the regular expression `pattern` matches in whole.
    #[must_use]
    pub fn any_matching(pattern: &str) -> SourceRequest {
        SourceRequest {
            path: SourcePath::Pattern(pattern.to_owned()),
            ..SourceRequest::any("")
        }
    }

    /// Only the requests whose query parameter `name` is `value`.
    #[must_use]
    pub fn query(mut self, name: &str, value: impl fmt::Display) -> SourceRequest {
        self.query.push(QueryCondition::Is(name.to_owned(), value.to_string()));
        self
    }

    /// Only the requests whose query parameter `name` holds `part`.
    #[must_use]
    pub fn query_containing(mut self, name: &str, part: impl fmt::Display) -> SourceRequest {
        self.query.push(QueryCondition::Contains(name.to_owned(), part.to_string()));
        self
    }

    /// Only the requests without the query parameter `name`.
    #[must_use]
    pub fn without_query(mut self, name: &str) -> SourceRequest {
        self.query.push(QueryCondition::Absent(name.to_owned()));
        self
    }

    /// Only the requests whose header fields meet `condition`.
    #[must_use]
    pub fn header(mut self, condition: HeaderCondition) -> SourceRequest {
        self.headers.push(condition);
        self
    }

    /// Only the requests whose body meets `pattern`.
    #[must_use]
    pub fn body(mut self, pattern: BodyPattern) -> SourceRequest {
        self.body.push(pattern);
        self
    }

    /// The answer's priority: 1 is tried before the source's other answers.
    #[must_use]
    pub const fn priority(mut self, priority: u8) -> SourceRequest {
        self.priority = Some(priority);
        self
    }

    /// Whether `request` is one of these requests.
    pub(crate) fn holds(&self, request: &Request) -> Result<bool, SourceError> {
        let path_holds = match &self.path {
            SourcePath::Exact(exact) => request.url.path() == exact,
            SourcePath::Pattern(pattern) => Regex::new(&SourcePath::anchored(pattern))
                .map_err(SourceError::Pattern)?
                .is_match(request.url.path()),
        };
        Ok(path_holds
            && self.verb.as_ref().is_none_or(|verb| request.method == *verb)
            && self.query.iter().all(|condition| condition.holds(request))
            && self.headers.iter().all(|condition| condition.holds(request))
            && self.body.iter().all(|pattern| pattern.holds(&request.body)))
    }

    /// The mock answering these requests with `answer`.
    pub(crate) fn mock(&self, answer: ResponseTemplate) -> Mock {
        let builder = match &self.path {
            SourcePath::Exact(exact) => Mock::given(path(exact.as_str())),
            SourcePath::Pattern(pattern) => Mock::given(path_regex(SourcePath::anchored(pattern))),
        };
        let builder = match &self.verb {
            Some(verb) => builder.and(method(verb.as_str())),
            None => builder,
        };
        let builder = self.query.iter().fold(builder, |builder, condition| match condition {
            QueryCondition::Is(name, value) => builder.and(query_param(name.as_str(), value.as_str())),
            QueryCondition::Contains(name, part) => builder.and(query_param_contains(name.as_str(), part.as_str())),
            QueryCondition::Absent(name) => builder.and(query_param_is_missing(name.as_str())),
        });
        let builder = self.headers.iter().fold(builder, |builder, condition| builder.and(condition.clone()));
        let builder = self.body.iter().fold(builder, |builder, pattern| builder.and(pattern.clone()));
        let mock = builder.respond_with(answer);
        match self.priority {
            Some(priority) => mock.with_priority(priority),
            None => mock,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{body_pattern::BodyPattern, source_request::SourceRequest};
    use googletest::prelude::*;
    use http::Method;
    use url::Url;
    use wiremock::Request;

    fn request(verb: Method, url: &str, body: &str) -> Result<Request> {
        Ok(Request {
            url: Url::parse(url)?,
            method: verb,
            headers: http::HeaderMap::new(),
            body: body.as_bytes().to_vec(),
        })
    }

    #[gtest]
    fn a_pattern_matches_the_whole_path_only() -> Result<()> {
        let forwarded = request(Method::GET, "http://source.test/ngsi-ld/v1/entities/x", "")?;
        expect_that!(SourceRequest::any_matching("/ngsi-ld/v1/entit.*").holds(&forwarded)?, eq(true));
        verify_that!(SourceRequest::any_matching("/entit.*").holds(&forwarded)?, eq(false))
    }

    #[gtest]
    fn a_method_query_and_body_query_narrow_the_requests() -> Result<()> {
        let forwarded = request(Method::POST, "http://source.test/a?type=T", r#"[{"id":"urn:ngsi-ld:V:1"}]"#)?;
        let described = SourceRequest::new(Method::POST, "/a")
            .query("type", "T")
            .body(BodyPattern::Selects("$[?(@.id == 'urn:ngsi-ld:V:1')]".parse()?));
        expect_that!(described.holds(&forwarded)?, eq(true));
        expect_that!(SourceRequest::new(Method::GET, "/a").holds(&forwarded)?, eq(false));
        verify_that!(
            SourceRequest::any("/a").body(BodyPattern::DoesNotContain("V:1".to_owned())).holds(&forwarded)?,
            eq(false)
        )
    }
}
