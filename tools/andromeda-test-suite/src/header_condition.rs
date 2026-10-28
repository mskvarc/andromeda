//! A requirement on one header field of a forwarded request, as a stub mapping of a source states it under `headers`:
//! the field is a value, holds a text, is one line a pattern matches, or is absent. Field names compare
//! case-insensitively (RFC 9110 section 5.1), values as written; a field on several lines meets a value requirement
//! when one of its lines does.

use http::{HeaderMap, HeaderName};
use regex::Regex;
use wiremock::{Match, Request};

/// A requirement on one header field of a request.
#[derive(Debug, Clone)]
pub enum HeaderCondition {
    /// One line of the field is the value.
    Is(HeaderName, String),
    /// One line of the field holds the text.
    Contains(HeaderName, String),
    /// The field is one line, which the pattern matches whole, as a stub mapping's `hasExactly` with one `matches`
    /// states; built by [`HeaderCondition::exactly`], which anchors the pattern.
    Exactly(HeaderName, Regex),
    /// The request carries no such field.
    Absent(HeaderName),
}

impl HeaderCondition {
    /// The name of the field the condition reads.
    const fn name(&self) -> &HeaderName {
        match self {
            HeaderCondition::Is(name, _) | HeaderCondition::Contains(name, _) | HeaderCondition::Exactly(name, _) | HeaderCondition::Absent(name) => name,
        }
    }

    /// The field `name` is one line the regular expression `pattern` matches whole.
    ///
    /// # Errors
    ///
    /// Returns the [`regex::Error`] of a pattern that is not a regular expression.
    pub fn exactly(name: HeaderName, pattern: &str) -> Result<HeaderCondition, regex::Error> {
        Regex::new(&format!("^(?:{pattern})$")).map(|anchored| HeaderCondition::Exactly(name, anchored))
    }

    /// Whether `request` meets the condition.
    #[must_use]
    pub fn holds(&self, request: &Request) -> bool {
        self.holds_in(&request.headers)
    }

    /// Whether the header fields `headers` meet the condition.
    #[must_use]
    pub fn holds_in(&self, headers: &HeaderMap) -> bool {
        let mut lines = headers.get_all(self.name()).iter().map(|value| value.to_str().ok());
        match self {
            HeaderCondition::Is(_, expected) => lines.any(|line| line == Some(expected.as_str())),
            HeaderCondition::Contains(_, part) => lines.any(|line| line.is_some_and(|line| line.contains(part.as_str()))),
            HeaderCondition::Exactly(_, pattern) => match (lines.next(), lines.next()) {
                (Some(Some(line)), None) => pattern.is_match(line),
                (Some(_) | None, _) => false,
            },
            HeaderCondition::Absent(name) => !headers.contains_key(name),
        }
    }
}

impl Match for HeaderCondition {
    fn matches(&self, request: &Request) -> bool {
        self.holds(request)
    }
}

#[cfg(test)]
mod tests {
    use crate::header_condition::HeaderCondition;
    use googletest::prelude::*;
    use http::{
        HeaderMap,
        HeaderName,
        HeaderValue,
        Method,
        header::{LINK, VIA},
    };
    use url::Url;
    use wiremock::Request;

    /// A request carrying `Via: 1.1 broker-a`.
    fn forwarded() -> Result<Request> {
        let mut headers = HeaderMap::new();
        headers.append(VIA, HeaderValue::from_static("1.1 broker-a"));
        Ok(Request {
            url: Url::parse("http://source.test/ngsi-ld/v1/entities")?,
            method: Method::GET,
            headers,
            body: Vec::new(),
        })
    }

    #[gtest]
    fn a_field_is_matched_by_value_by_text_and_by_absence() -> Result<()> {
        let request = forwarded()?;
        expect_that!(
            HeaderCondition::Is(HeaderName::from_static("via"), "1.1 broker-a".to_owned()).holds(&request),
            eq(true)
        );
        expect_that!(HeaderCondition::Contains(VIA, "broker-a".to_owned()).holds(&request), eq(true));
        expect_that!(HeaderCondition::Contains(VIA, "broker-b".to_owned()).holds(&request), eq(false));
        expect_that!(HeaderCondition::Absent(LINK).holds(&request), eq(true));
        verify_that!(HeaderCondition::Absent(VIA).holds(&request), eq(false))
    }

    #[gtest]
    fn exactly_requires_one_line_the_pattern_matches() -> Result<()> {
        let request = forwarded()?;
        expect_that!(HeaderCondition::exactly(VIA, "1\\.1 broker-a")?.holds(&request), eq(true));
        expect_that!(HeaderCondition::exactly(VIA, "broker-a")?.holds(&request), eq(false));
        let mut twice = HeaderMap::new();
        twice.append(VIA, HeaderValue::from_static("1.1 broker-a"));
        twice.append(VIA, HeaderValue::from_static("1.1 broker-a"));
        expect_that!(HeaderCondition::exactly(VIA, ".*broker-a")?.holds_in(&twice), eq(false));
        verify_that!(HeaderCondition::exactly(LINK, ".*")?.holds(&request), eq(false))
    }
}
