//! A matcher on the `NGSILD-Warning` header (clause 6.3.17) of an answer of any status, read through the typed header:
//! a distributed read the broker could not complete wholly answers a success or an error, and either carries the
//! warning.

use crate::answered_status::AnsweredStatus;
use googletest::{
    description::Description,
    matcher::{Matcher, MatcherBase, MatcherResult},
};
use ngsi_ld::http::{
    ResponseHead,
    headers::{NgsiLdWarnings, WarningCode},
};
use std::fmt::Debug;

/// Which of the warnings must carry the code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Position {
    /// Any warning of the header.
    Any,
    /// The first warning of the header.
    First,
}

/// An answer whose `NGSILD-Warning` header carries a warning code.
#[derive(Debug, Clone, Copy, MatcherBase)]
pub struct WarnsWith {
    /// The code required.
    code: WarningCode,
    /// Which warning must carry it.
    position: Position,
}

/// An answer with a warning of `code` among the warnings of its `NGSILD-Warning` header.
#[must_use]
pub const fn warns_with(code: WarningCode) -> WarnsWith {
    WarnsWith { code, position: Position::Any }
}

/// An answer whose `NGSILD-Warning` header starts with a warning of `code`.
#[must_use]
pub const fn warns_first_with(code: WarningCode) -> WarnsWith {
    WarnsWith {
        code,
        position: Position::First,
    }
}

impl<A: AnsweredStatus + Debug> Matcher<&A> for WarnsWith {
    fn matches(&self, actual: &A) -> MatcherResult {
        let warns = actual.answered_headers().is_some_and(|headers| {
            ResponseHead::new(headers.clone())
                .typed::<NgsiLdWarnings>()
                .valid()
                .is_some_and(|warnings| match self.position {
                    Position::Any => warnings.iter().any(|warning| warning.code() == self.code),
                    Position::First => warnings.iter().next().is_some_and(|warning| warning.code() == self.code),
                })
        });
        warns.into()
    }

    fn describe(&self, result: MatcherResult) -> Description {
        let place = match self.position {
            Position::Any => "a",
            Position::First => "a first",
        };
        match result {
            MatcherResult::Match => format!("carries {place} NGSILD-Warning of code {}", self.code).into(),
            MatcherResult::NoMatch => format!("carries no {place} NGSILD-Warning of code {}", self.code).into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::answer_warning::{warns_first_with, warns_with};
    use bytes::Bytes;
    use googletest::prelude::*;
    use http::{HeaderValue, Response};
    use ngsi_ld::{client::ClientError, http::headers::WarningCode};
    use std::{convert::Infallible, result::Result as StdResult};

    /// An answer holding the `NGSILD-Warning` field lines `lines`.
    #[expect(clippy::unnecessary_wraps, reason = "the matchers read the Result a client call answers")]
    fn answer(lines: &[&'static str]) -> StdResult<Response<Bytes>, ClientError<Infallible>> {
        let mut response = Response::new(Bytes::new());
        for line in lines.iter().copied() {
            response.headers_mut().append("ngsild-warning", HeaderValue::from_static(line));
        }
        Ok(response)
    }

    #[gtest]
    fn a_warning_is_found_among_the_lines_of_the_header() -> Result<()> {
        let held = answer(&["299 - Miscellaneous Persistent Warning", "199 - Miscellaneous Warning"]);
        expect_that!(&held, warns_with(WarningCode::MiscellaneousWarning));
        verify_that!(&held, not(warns_with(WarningCode::ResponseIsStale)))
    }

    #[gtest]
    fn the_first_warning_is_the_one_a_header_starts_with() -> Result<()> {
        let held = answer(&["299 - Miscellaneous Persistent Warning", "199 - Miscellaneous Warning"]);
        expect_that!(&held, warns_first_with(WarningCode::MiscellaneousPersistentWarning));
        verify_that!(&held, not(warns_first_with(WarningCode::MiscellaneousWarning)))
    }

    #[gtest]
    fn an_answer_without_the_header_carries_no_warning() -> Result<()> {
        verify_that!(&answer(&[]), not(warns_with(WarningCode::MiscellaneousWarning)))
    }
}
