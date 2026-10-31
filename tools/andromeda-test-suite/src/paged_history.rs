//! The long histories of the page-walking tests: a `PagedSeries` entity whose `speed` (and `fuel`) instances sit at
//! minutes of 2024-01-01, the pages of such a history read from the answers the walk requests, and the `Content-Range`
//! (clause 6.3.10) a partial page carries.

use crate::namespace::{Namespace, NamespaceError};
use bytes::Bytes;
use googletest::{
    description::Description,
    matcher::{Matcher, MatcherBase, MatcherResult},
};
use http::Response;
use ngsi_ld::{
    http::headers::{ContentRange, ContentRangeSize},
    model::{BuildError, DateTime, DocumentError, Input, Lossless, Output, Property, TemporalError, evolution::TemporalEntity},
};
use serde_json::Value;
use std::str::{self, Utf8Error};

/// Why a history could not be built or a page not read.
#[derive(Debug, thiserror::Error)]
pub enum PagedHistoryError {
    /// A name or identifier is invalid.
    #[error(transparent)]
    Namespace(#[from] NamespaceError),
    /// An instant is invalid.
    #[error(transparent)]
    Instant(#[from] TemporalError),
    /// A member of the history is invalid.
    #[error(transparent)]
    Build(#[from] BuildError),
    /// The body of a page is not UTF-8.
    #[error("the body of the page is not UTF-8")]
    Utf8(#[from] Utf8Error),
    /// The body of a page is not JSON.
    #[error("the body of the page is not JSON")]
    Json(#[from] serde_json::Error),
    /// The body of a page is not a temporal entity.
    #[error("the body of the page is not a temporal entity")]
    Document(#[from] DocumentError),
}

/// The instant `minutes` minutes after 2024-01-01T00:00:00Z.
///
/// # Errors
///
/// Returns [`PagedHistoryError::Instant`] when the text is not an instant, which a count of minutes of a few days never
/// is.
pub fn stamp(minutes: u32) -> Result<DateTime, PagedHistoryError> {
    Ok(format!("2024-01-0{}T{:02}:{:02}:00Z", 1 + minutes / 1440, minutes % 1440 / 60, minutes % 60).parse()?)
}

/// The entity `urn:ngsi-ld:PagedSeries:<token>-<suffix>` with a `speed` instance for each `(value, minutes)` of `speed`
/// and a `fuel` instance for each of `fuel`, each observed at its minutes (see [`stamp`]).
///
/// # Errors
///
/// Returns [`PagedHistoryError`] when a name, an instant or a member is invalid.
pub fn series(namespace: &Namespace, suffix: &str, speed: &[(i32, u32)], fuel: &[(i32, u32)]) -> Result<TemporalEntity<Input>, PagedHistoryError> {
    let start = TemporalEntity::builder()
        .id(namespace.urn("PagedSeries", suffix)?)
        .entity_type(namespace.entity_type("PagedSeries")?);
    let with_speed = speed.iter().try_fold(start, |builder, (value, minutes)| -> Result<_, PagedHistoryError> {
        Ok(builder.property("speed", Property::builder(*value).observed_at(stamp(*minutes)?)))
    })?;
    let with_fuel = fuel.iter().try_fold(with_speed, |builder, (value, minutes)| -> Result<_, PagedHistoryError> {
        Ok(builder.property("fuel", Property::builder(*value).observed_at(stamp(*minutes)?)))
    })?;
    Ok(with_fuel.build()?)
}

/// The temporal entity a page answers, in the normalized representation.
///
/// An attribute whose instances all lie beyond the page is answered as an empty array, which the typed reader refuses,
/// so such a member is left out before the page is read.
///
/// # Errors
///
/// Returns [`PagedHistoryError`] when the body is not UTF-8, not JSON or not a temporal entity.
pub fn read(response: &Response<Bytes>) -> Result<TemporalEntity<Output>, PagedHistoryError> {
    let mut page: Value = serde_json::from_str(str::from_utf8(response.body())?)?;
    if let Value::Object(members) = &mut page {
        members.retain(|_, member| !matches!(member, Value::Array(items) if items.is_empty()));
    }
    Ok(TemporalEntity::<Output>::parse(&page.to_string(), Lossless::Normalized)?)
}

/// Matches a `Content-Range` of the instants `start` and `end` and the size `size`.
#[derive(Debug, Clone, MatcherBase)]
pub struct HasRange {
    /// The instant the range starts at.
    start: DateTime,
    /// The instant the range ends at.
    end: DateTime,
    /// The size of the range.
    size: ContentRangeSize,
}

/// A `Content-Range` from `start` to `end` of `size`, the instants compared as instants.
#[must_use]
pub const fn has_range(start: DateTime, end: DateTime, size: ContentRangeSize) -> HasRange {
    HasRange { start, end, size }
}

impl Matcher<&ContentRange> for HasRange {
    fn matches(&self, actual: &ContentRange) -> MatcherResult {
        (actual.start().timestamp() == self.start.timestamp() && actual.end().timestamp() == self.end.timestamp() && actual.size() == self.size).into()
    }

    fn describe(&self, result: MatcherResult) -> Description {
        match result {
            MatcherResult::Match => format!("is the range {} to {} of {:?}", self.start, self.end, self.size).into(),
            MatcherResult::NoMatch => format!("isn't the range {} to {} of {:?}", self.start, self.end, self.size).into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        namespace::Namespace,
        paged_history::{read, series, stamp},
    };
    use bytes::Bytes;
    use googletest::prelude::*;
    use http::Response;
    use ngsi_ld::model::DateTime;

    #[gtest]
    fn a_count_of_minutes_is_an_instant_of_the_first_days_of_2024() -> Result<()> {
        expect_that!(stamp(0)?.timestamp(), eq("2024-01-01T00:00:00Z".parse::<DateTime>()?.timestamp()));
        expect_that!(stamp(99)?.timestamp(), eq("2024-01-01T01:39:00Z".parse::<DateTime>()?.timestamp()));
        verify_that!(stamp(1440 + 61)?.timestamp(), eq("2024-01-02T01:01:00Z".parse::<DateTime>()?.timestamp()))
    }

    #[gtest]
    fn a_series_holds_the_instances_of_both_attributes() -> Result<()> {
        let namespace = Namespace::fresh();
        let entity = series(&namespace, "forward", &[(0, 0), (1, 1)], &[(100, 0)])?;
        verify_that!(entity.attributes.len(), eq(2))
    }

    #[gtest]
    fn a_page_with_an_empty_attribute_is_read_without_it() -> Result<()> {
        let page = read(&Response::new(Bytes::from_static(
            br#"{"id":"urn:ngsi-ld:PagedSeries:1","type":"PagedSeries","fuel":[],"speed":[{"type":"Property","value":1,"observedAt":"2024-01-01T00:00:00Z"}]}"#,
        )))?;
        verify_that!(page.attributes.len(), eq(1))
    }
}
