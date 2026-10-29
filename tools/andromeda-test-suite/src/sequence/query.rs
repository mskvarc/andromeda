//! What a check or a capture reads from an answer: a source (status, a header, the body, a JSON path) and the filters
//! applied to it in order.

use crate::sequence::{answer::Answer, peer_certificate::CertificateField, queried::Queried, query_error::QueryError, variables::Variables};
use chrono::{DateTime, NaiveDateTime, Utc};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use regex::Regex;
use serde_json::Value as Json;
use serde_json_path::JsonPath;
use std::{
    fmt::{self, Display},
    str,
};

/// The characters `urlEncode` keeps as they are: the unreserved characters of RFC 3986 and `/`.
const URL_ENCODE_KEPT: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'.').remove(b'_').remove(b'~').remove(b'/');

/// Where a query reads.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Source {
    /// The status code.
    Status,
    /// Every value of a header.
    Header(String),
    /// The body as text.
    Body,
    /// The body as bytes.
    Bytes,
    /// The nodes a JSON path selects in the body.
    JsonPath(String),
    /// The time the answer took, in milliseconds.
    Duration,
    /// The URL asked.
    Url,
    /// A field of the certificate the server presented.
    Certificate(CertificateField),
}

/// A filter, applied to what the source or the previous filter yields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Filter {
    /// The number of items of a list or bytes of a body.
    Count,
    /// The nodes a JSON path selects in the JSON text of a string.
    JsonPath(String),
    /// The first group a regular expression captures in a string, or the whole match when it has no group.
    Regex(String),
    /// Every match of a regular expression replaced; the replacement is a template.
    ReplaceRegex(String, String),
    /// Every occurrence of a text replaced; both are templates.
    Replace(String, String),
    /// The parts of a string between occurrences of a separator.
    Split(String),
    /// The item at an index of a list.
    Nth(usize),
    /// The integer a string or number holds.
    ToInt,
    /// The instant a string holds, in a `strftime` format.
    ToDate(String),
    /// An instant written in a `strftime` format.
    Format(String),
    /// The whole days from now to an instant.
    DaysAfterNow,
    /// A string percent-encoded as a URL component, `/` kept.
    UrlEncode,
}

/// A source and its filters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    /// Where it reads.
    source: Source,
    /// The filters, in order.
    filters: Vec<Filter>,
}

/// The status code.
#[must_use]
pub const fn status_code() -> Query {
    Query::of(Source::Status)
}

/// Every value of the header `name`, matched without regard to case.
#[must_use]
pub fn header(name: &str) -> Query {
    Query::of(Source::Header(name.to_owned()))
}

/// The body as text.
#[must_use]
pub const fn body() -> Query {
    Query::of(Source::Body)
}

/// The body as bytes.
#[must_use]
pub const fn bytes() -> Query {
    Query::of(Source::Bytes)
}

/// The nodes `expression` selects in the JSON body.
#[must_use]
pub fn json(expression: &str) -> Query {
    Query::of(Source::JsonPath(expression.to_owned()))
}

/// The time the answer took, in milliseconds.
#[must_use]
pub const fn duration() -> Query {
    Query::of(Source::Duration)
}

/// The URL asked.
#[must_use]
pub const fn url() -> Query {
    Query::of(Source::Url)
}

/// The field `field` of the certificate the server presented.
#[must_use]
pub const fn certificate(field: CertificateField) -> Query {
    Query::of(Source::Certificate(field))
}

impl Query {
    /// The query of `source`, unfiltered.
    const fn of(source: Source) -> Query {
        Query { source, filters: Vec::new() }
    }

    /// The query with `filter` applied last.
    fn then(mut self, filter: Filter) -> Query {
        self.filters.push(filter);
        self
    }

    /// The number of items or bytes.
    #[must_use]
    pub fn count(self) -> Query {
        self.then(Filter::Count)
    }

    /// The nodes `expression` selects in the JSON text the query yields.
    #[must_use]
    pub fn json(self, expression: &str) -> Query {
        self.then(Filter::JsonPath(expression.to_owned()))
    }

    /// The first group `pattern` captures, or its whole match.
    #[must_use]
    pub fn regex(self, pattern: &str) -> Query {
        self.then(Filter::Regex(pattern.to_owned()))
    }

    /// Every match of `pattern` replaced with the template `replacement`.
    #[must_use]
    pub fn replace_regex(self, pattern: &str, replacement: &str) -> Query {
        self.then(Filter::ReplaceRegex(pattern.to_owned(), replacement.to_owned()))
    }

    /// Every occurrence of the template `old` replaced with the template `new`.
    #[must_use]
    pub fn replace(self, old: &str, new: &str) -> Query {
        self.then(Filter::Replace(old.to_owned(), new.to_owned()))
    }

    /// The parts between occurrences of the template `separator`.
    #[must_use]
    pub fn split(self, separator: &str) -> Query {
        self.then(Filter::Split(separator.to_owned()))
    }

    /// The item at `index`.
    #[must_use]
    pub fn nth(self, index: usize) -> Query {
        self.then(Filter::Nth(index))
    }

    /// The integer the value holds.
    #[must_use]
    pub fn to_int(self) -> Query {
        self.then(Filter::ToInt)
    }

    /// The instant the value holds, written in the `strftime` `format`.
    #[must_use]
    pub fn to_date(self, format: &str) -> Query {
        self.then(Filter::ToDate(format.to_owned()))
    }

    /// The instant written in the `strftime` `format`.
    #[must_use]
    pub fn format(self, format: &str) -> Query {
        self.then(Filter::Format(format.to_owned()))
    }

    /// The whole days from now to the instant.
    #[must_use]
    pub fn days_after_now(self) -> Query {
        self.then(Filter::DaysAfterNow)
    }

    /// The value percent-encoded as a URL component.
    #[must_use]
    pub fn url_encode(self) -> Query {
        self.then(Filter::UrlEncode)
    }

    /// What the query yields on `answer`.
    ///
    /// # Errors
    ///
    /// Returns [`QueryError`] when the source or a filter cannot read what it is given.
    pub fn evaluate(&self, answer: &Answer, variables: &Variables) -> Result<Queried, QueryError> {
        let source = self.source.read(answer, variables)?;
        self.filters.iter().try_fold(source, |value, filter| filter.apply(value, variables))
    }
}

impl Source {
    /// What the source yields on `answer`, its expression's templates filled from `variables`.
    fn read(&self, answer: &Answer, variables: &Variables) -> Result<Queried, QueryError> {
        match self {
            Source::Status => Ok(Queried::Integer(i64::from(answer.status))),
            Source::Header(name) => {
                let values = answer
                    .headers
                    .get_all(variables.fill(name)?.as_str())
                    .iter()
                    .map(|value| Queried::String(String::from_utf8_lossy(value.as_bytes()).into_owned()))
                    .collect::<Vec<_>>();
                Ok(match <[Queried; 1]>::try_from(values) {
                    Ok([single]) => single,
                    Err(values) if values.is_empty() => Queried::Nothing,
                    Err(values) => Queried::List(values),
                })
            }
            Source::Body => text(answer).map(|body| Queried::String(body.to_owned())),
            Source::Bytes => Ok(Queried::Bytes(answer.body.clone())),
            Source::JsonPath(expression) => {
                let document = serde_json::from_str::<Json>(text(answer)?).map_err(QueryError::BodyNotJson)?;
                select(&variables.fill(expression)?, &document)
            }
            Source::Duration => Ok(Queried::Integer(i64::try_from(answer.duration.as_millis()).unwrap_or(i64::MAX))),
            Source::Url => Ok(Queried::String(answer.url.clone())),
            Source::Certificate(field) => Ok(answer
                .certificate
                .as_ref()
                .map_or(Queried::Nothing, |certificate| Queried::String(certificate.field(*field).to_owned()))),
        }
    }
}

/// The body of `answer` as text.
fn text(answer: &Answer) -> Result<&str, QueryError> {
    str::from_utf8(&answer.body).map_err(|_| QueryError::NotText)
}

/// The nodes `expression` selects in `document`.
fn select(expression: &str, document: &Json) -> Result<Queried, QueryError> {
    let path = JsonPath::parse(expression).map_err(QueryError::Expression)?;
    Ok(Queried::of_nodes(&path.query(document).all()))
}

/// The regular expression `pattern`.
fn pattern(pattern: &str) -> Result<Regex, QueryError> {
    Regex::new(pattern).map_err(QueryError::Pattern)
}

impl Filter {
    /// What the filter makes of `value`.
    fn apply(&self, value: Queried, variables: &Variables) -> Result<Queried, QueryError> {
        // clone: the error names the filter the query keeps
        let unreadable = |value: Queried| QueryError::Unreadable {
            filter: Box::new(self.clone()),
            value: Box::new(value),
        };
        match (self, value) {
            (Filter::Count, Queried::List(items)) => Ok(Queried::Integer(i64::try_from(items.len()).unwrap_or(i64::MAX))),
            (Filter::Count, Queried::Bytes(bytes)) => Ok(Queried::Integer(i64::try_from(bytes.len()).unwrap_or(i64::MAX))),
            (Filter::JsonPath(expression), Queried::String(json)) => {
                let document = match serde_json::from_str::<Json>(&json) {
                    Ok(document) => document,
                    Err(source) => {
                        return Err(QueryError::ValueNotJson {
                            value: Box::new(Queried::String(json)),
                            source,
                        });
                    }
                };
                select(&variables.fill(expression)?, &document)
            }
            (Filter::JsonPath(expression), value @ (Queried::Object(_) | Queried::List(_))) => {
                let Some(document) = value.to_json() else {
                    return Err(unreadable(value));
                };
                select(&variables.fill(expression)?, &document)
            }
            (Filter::Regex(expression), Queried::String(subject)) => Ok(pattern(&variables.fill(expression)?)?
                .captures(&subject)
                .and_then(|captures| captures.get(1).or_else(|| captures.get(0)))
                .map_or(Queried::Nothing, |found| Queried::String(found.as_str().to_owned()))),
            (Filter::ReplaceRegex(expression, replacement), Queried::String(subject)) => {
                let replacement = variables.fill(replacement)?;
                Ok(Queried::String(
                    pattern(&variables.fill(expression)?)?.replace_all(&subject, replacement.as_str()).into_owned(),
                ))
            }
            (Filter::Replace(old, new), Queried::String(subject)) => Ok(Queried::String(subject.replace(&variables.fill(old)?, &variables.fill(new)?))),
            (Filter::Split(separator), Queried::String(subject)) => {
                let separator = variables.fill(separator)?;
                Ok(Queried::List(
                    subject.split(separator.as_str()).map(|part| Queried::String(part.to_owned())).collect(),
                ))
            }
            (Filter::Nth(index), Queried::List(items)) => {
                let length = items.len();
                items.into_iter().nth(*index).ok_or_else(|| QueryError::PastTheEnd {
                    // clone: the error names the filter the query keeps
                    filter: Box::new(self.clone()),
                    length,
                })
            }
            (Filter::ToInt, Queried::String(digits)) => digits
                .trim()
                .parse::<i64>()
                .map(Queried::Integer)
                .map_err(|_| unreadable(Queried::String(digits))),
            (Filter::ToInt, Queried::Integer(integer)) => Ok(Queried::Integer(integer)),
            #[expect(clippy::cast_possible_truncation, reason = "toInt truncates a float toward zero")]
            (Filter::ToInt, Queried::Float(float)) => Ok(Queried::Integer(float.trunc() as i64)),
            (Filter::ToDate(format), Queried::String(written)) => NaiveDateTime::parse_from_str(&written, format)
                .map(|naive| Queried::Date(naive.and_utc()))
                .or_else(|_| DateTime::parse_from_str(&written, format).map(|instant| Queried::Date(instant.with_timezone(&Utc))))
                .map_err(|_| unreadable(Queried::String(written))),
            (Filter::Format(format), Queried::Date(instant)) => Ok(Queried::String(instant.format(format).to_string())),
            (Filter::DaysAfterNow, Queried::Date(instant)) => Ok(Queried::Integer((instant - Utc::now()).num_days())),
            (Filter::UrlEncode, Queried::String(subject)) => Ok(Queried::String(utf8_percent_encode(&subject, URL_ENCODE_KEPT).to_string())),
            (_, value) => Err(unreadable(value)),
        }
    }
}

impl Display for Filter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Filter::Count => write!(formatter, "count"),
            Filter::JsonPath(expression) => write!(formatter, "jsonpath {expression:?}"),
            Filter::Regex(expression) => write!(formatter, "regex {expression:?}"),
            Filter::ReplaceRegex(expression, replacement) => write!(formatter, "replaceRegex {expression:?} {replacement:?}"),
            Filter::Replace(old, new) => write!(formatter, "replace {old:?} {new:?}"),
            Filter::Split(separator) => write!(formatter, "split {separator:?}"),
            Filter::Nth(index) => write!(formatter, "nth {index}"),
            Filter::ToInt => write!(formatter, "toInt"),
            Filter::ToDate(format) => write!(formatter, "toDate {format:?}"),
            Filter::Format(format) => write!(formatter, "format {format:?}"),
            Filter::DaysAfterNow => write!(formatter, "daysAfterNow"),
            Filter::UrlEncode => write!(formatter, "urlEncode"),
        }
    }
}

impl Display for Query {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.source {
            Source::Status => write!(formatter, "status")?,
            Source::Header(name) => write!(formatter, "header {name:?}")?,
            Source::Body => write!(formatter, "body")?,
            Source::Bytes => write!(formatter, "bytes")?,
            Source::JsonPath(expression) => write!(formatter, "jsonpath {expression:?}")?,
            Source::Duration => write!(formatter, "duration")?,
            Source::Url => write!(formatter, "url")?,
            Source::Certificate(field) => write!(formatter, "certificate {:?}", field.to_string())?,
        }
        self.filters.iter().try_for_each(|filter| write!(formatter, " {filter}"))
    }
}

#[cfg(test)]
mod tests {
    use crate::sequence::{
        answer::Answer,
        queried::Queried,
        query::{header, json},
        variables::Variables,
    };
    use googletest::prelude::*;
    use http::{HeaderMap, HeaderValue};

    fn answer(body: &str) -> Answer {
        Answer::json(200, body.to_owned(), "http://broker/".to_owned())
    }

    #[gtest]
    fn a_wildcard_over_one_item_yields_the_item() -> Result<()> {
        let value = json("$.one[*]").evaluate(&answer(r#"{"one": [7]}"#), &Variables::default())?;
        verify_that!(value, eq(&Queried::Integer(7)))
    }

    #[gtest]
    fn a_filter_selecting_nothing_yields_nothing() -> Result<()> {
        let value = json("$.objs[?@.v==9]").evaluate(&answer(r#"{"objs": [{"v": 1}]}"#), &Variables::default())?;
        verify_that!(value, eq(&Queried::Nothing))
    }

    #[gtest]
    fn count_of_a_string_is_an_error() {
        expect_that!(json("$.s").count().evaluate(&answer(r#"{"s": "abc"}"#), &Variables::default()), err(anything()));
    }

    #[gtest]
    fn a_json_path_filter_reads_a_string_as_json() -> Result<()> {
        let value = json("$.body")
            .json("$.n")
            .evaluate(&answer(r#"{"body": "{\"n\": 2}"}"#), &Variables::default())?;
        verify_that!(value, eq(&Queried::Integer(2)))
    }

    #[gtest]
    fn a_header_sent_twice_is_a_list() -> Result<()> {
        let mut found = answer("{}");
        found.headers = HeaderMap::new();
        found.headers.append("ngsild-warning", HeaderValue::from_static("a"));
        found.headers.append("ngsild-warning", HeaderValue::from_static("b"));
        verify_that!(
            header("NGSILD-Warning").count().evaluate(&found, &Variables::default())?,
            eq(&Queried::Integer(2))
        )
    }

    #[gtest]
    fn a_regex_without_match_yields_nothing() -> Result<()> {
        let value = json("$.u").regex("zzz(.)").evaluate(&answer(r#"{"u": "abc"}"#), &Variables::default())?;
        verify_that!(value, eq(&Queried::Nothing))
    }

    #[gtest]
    fn url_encode_keeps_unreserved_characters_and_slashes() -> Result<()> {
        let value = json("$.u")
            .url_encode()
            .evaluate(&answer(r#"{"u": "urn:a b/c?d=e&f+g~h"}"#), &Variables::default())?;
        verify_that!(value, eq(&Queried::String("urn%3Aa%20b/c%3Fd%3De%26f%2Bg~h".to_owned())))
    }

    #[gtest]
    fn a_date_round_trips_through_microseconds() -> Result<()> {
        let value = json("$.d")
            .to_date("%Y-%m-%dT%H:%M:%S%.fZ")
            .format("%s%6f")
            .to_int()
            .evaluate(&answer(r#"{"d": "2026-10-26T21:53:05.107Z"}"#), &Variables::default())?;
        verify_that!(value, eq(&Queried::Integer(1_793_051_585_107_000)))
    }
}
