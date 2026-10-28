//! Matchers on a typed response header (`OptionalHeader`): whether the broker sent it and whether it read as the
//! header's grammar requires.
//!
//! A header the broker sent but that does not decode is neither absent nor present here, so a test that requires a
//! header to be missing fails on a malformed one as well.

use googletest::{
    description::Description,
    matcher::{Matcher, MatcherBase, MatcherResult},
};
use ngsi_ld::http::headers::OptionalHeader;
use std::fmt::Debug;

/// What a header is required to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Required {
    /// Not sent at all.
    Absent,
    /// Sent and read.
    Present,
}

/// The requirement on one header.
#[derive(Debug, Clone, Copy, MatcherBase)]
pub struct HeaderMatcher {
    /// What the header is required to be.
    required: Required,
}

/// A header the broker did not send.
#[must_use]
pub const fn absent() -> HeaderMatcher {
    HeaderMatcher { required: Required::Absent }
}

/// A header the broker sent, reading as its grammar requires.
#[must_use]
pub const fn present() -> HeaderMatcher {
    HeaderMatcher { required: Required::Present }
}

impl<T: Debug> Matcher<&OptionalHeader<T>> for HeaderMatcher {
    fn matches(&self, actual: &OptionalHeader<T>) -> MatcherResult {
        match (self.required, actual) {
            (Required::Absent, OptionalHeader::Absent) | (Required::Present, OptionalHeader::Valid(_)) => MatcherResult::Match,
            (Required::Absent, OptionalHeader::Valid(_) | OptionalHeader::Malformed(_))
            | (Required::Present, OptionalHeader::Absent | OptionalHeader::Malformed(_)) => MatcherResult::NoMatch,
        }
    }

    fn describe(&self, result: MatcherResult) -> Description {
        match (self.required, result) {
            (Required::Absent, MatcherResult::Match) => "is a header the broker did not send".into(),
            (Required::Absent, MatcherResult::NoMatch) => "is a header the broker sent".into(),
            (Required::Present, MatcherResult::Match) => "is a header the broker sent and that reads".into(),
            (Required::Present, MatcherResult::NoMatch) => "isn't a header the broker sent and that reads".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::header_matcher::{absent, present};
    use googletest::prelude::*;
    use http::HeaderValue;
    use ngsi_ld::http::headers::OptionalHeader;

    #[gtest]
    fn a_missing_header_is_absent_and_not_present() -> Result<()> {
        let header: OptionalHeader<u8> = OptionalHeader::Absent;
        expect_that!(&header, absent());
        verify_that!(&header, not(present()))
    }

    #[gtest]
    fn a_read_header_is_present_and_not_absent() -> Result<()> {
        let header = OptionalHeader::Valid(7_u8);
        expect_that!(&header, present());
        verify_that!(&header, not(absent()))
    }

    #[gtest]
    fn a_malformed_header_is_neither_absent_nor_present() -> Result<()> {
        let header: OptionalHeader<u8> = OptionalHeader::Malformed(vec![HeaderValue::from_static("?")]);
        expect_that!(&header, not(absent()));
        verify_that!(&header, not(present()))
    }
}
