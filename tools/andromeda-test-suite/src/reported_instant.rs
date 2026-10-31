//! An instant as an Andromeda extension document writes it: RFC 3339 with an offset, up to nanosecond digits.

use chrono::{DateTime, FixedOffset};
use serde::Deserialize;

/// An RFC 3339 instant read from a document no NGSI-LD model covers (the version report, the Tenant resource).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct ReportedInstant(pub DateTime<FixedOffset>);

impl TryFrom<String> for ReportedInstant {
    type Error = chrono::ParseError;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        DateTime::parse_from_rfc3339(&text).map(ReportedInstant)
    }
}

#[cfg(test)]
mod tests {
    use crate::reported_instant::ReportedInstant;
    use googletest::prelude::*;

    #[gtest]
    fn an_offset_and_nanosecond_digits_are_read() {
        expect_that!(
            serde_json::from_str::<ReportedInstant>(r#""2026-10-29T16:37:22.853856262+00:00""#),
            ok(anything())
        );
        expect_that!(serde_json::from_str::<ReportedInstant>(r#""2026-10-30T04:59:03.350386Z""#), ok(anything()));
    }

    #[gtest]
    fn text_that_is_no_instant_is_refused() {
        expect_that!(serde_json::from_str::<ReportedInstant>(r#""yesterday""#), err(anything()));
    }
}
