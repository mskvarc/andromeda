//! The `Retry-After` response header (RFC 9110 section 10.2.3).
//!
//! A server asks a client to wait before its next request, either as a number
//! of seconds counted from when the response is received or as the HTTP-date
//! before which the client is asked not to retry. The broker writes the first
//! form on a retryable 503 and reads either form on the answer of a
//! notification endpoint.

use http::{HeaderName, HeaderValue};
use httpdate::HttpDate;
use std::{
    iter,
    time::{Duration, SystemTime},
};

/// The lowercase `Retry-After` field name this header is transmitted under.
static RETRY_AFTER: HeaderName = HeaderName::from_static("retry-after");

/// How long a server asks a client to wait before its next request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryAfter {
    /// `delay-seconds`: a wait counted from when the response is received.
    ///
    /// It is written in whole seconds, rounded up, so the wait it asks for is
    /// never shorter than the one held. A value past the largest count of
    /// seconds a `u64` holds is read as that count, as RFC 9111 section 1.2.2
    /// reads an overflowing delta-seconds.
    Delay(Duration),
    /// An `HTTP-date`: the instant before which the client is asked not to
    /// retry. All three formats RFC 9110 section 5.6.7 has a recipient accept
    /// are read, and the IMF-fixdate is written.
    Date(HttpDate),
}

impl RetryAfter {
    /// The wait the header asks for, counted from `received`, the instant the
    /// response arrived: a delay as it is, and the time from `received` to a
    /// date, which is no wait at all for a date at or before it.
    #[must_use]
    pub fn wait_from(self, received: SystemTime) -> Duration {
        match self {
            RetryAfter::Delay(delay) => delay,
            RetryAfter::Date(date) => SystemTime::from(date).duration_since(received).unwrap_or(Duration::ZERO),
        }
    }

    /// Read a field value: one or more digits are a delay, anything else is
    /// read as a date. `None` when it is neither.
    fn parse(text: &str) -> Option<RetryAfter> {
        if !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit()) {
            Some(RetryAfter::Delay(Duration::from_secs(text.parse().unwrap_or(u64::MAX))))
        } else {
            text.parse().ok().map(RetryAfter::Date)
        }
    }
}

/// `delay` in whole seconds, rounded up.
fn whole_seconds(delay: Duration) -> u64 {
    delay.as_secs().saturating_add(u64::from(delay.subsec_nanos() > 0))
}

impl headers::Header for RetryAfter {
    fn name() -> &'static HeaderName {
        &RETRY_AFTER
    }
    fn decode<'i, I>(values: &mut I) -> Result<Self, headers::Error>
    where
        I: Iterator<Item = &'i HeaderValue>,
    {
        let value = values.next().ok_or_else(headers::Error::invalid)?;
        value.to_str().ok().and_then(RetryAfter::parse).ok_or_else(headers::Error::invalid)
    }
    fn encode<E: Extend<HeaderValue>>(&self, values: &mut E) {
        match self {
            RetryAfter::Delay(delay) => values.extend(iter::once(HeaderValue::from(whole_seconds(*delay)))),
            RetryAfter::Date(date) => {
                if let Ok(value) = HeaderValue::from_str(&date.to_string()) {
                    values.extend(iter::once(value));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::retry_after::RetryAfter;
    use headers::{Header, HeaderMapExt};
    use http::{HeaderMap, HeaderValue};
    use httpdate::HttpDate;
    use pretty_assertions::assert_eq;
    use std::{
        iter,
        time::{Duration, SystemTime},
    };

    fn decoded(value: &HeaderValue) -> Option<RetryAfter> {
        RetryAfter::decode(&mut iter::once(value)).ok()
    }

    fn read(text: &'static str) -> Option<RetryAfter> {
        decoded(&HeaderValue::from_static(text))
    }

    fn written(header: RetryAfter) -> Option<HeaderValue> {
        let mut headers = HeaderMap::new();
        headers.typed_insert(header);
        headers.get("retry-after").cloned()
    }

    /// The example date of RFC 9110 section 5.6.7.
    fn example_date() -> HttpDate {
        "Sun, 06 Nov 1994 08:49:37 GMT".parse().expect("an IMF-fixdate")
    }

    #[test]
    fn a_count_of_seconds_is_read_as_that_delay() {
        assert_eq!(read("120"), Some(RetryAfter::Delay(Duration::from_secs(120))));
        assert_eq!(read("0"), Some(RetryAfter::Delay(Duration::ZERO)));
    }

    #[test]
    fn a_count_of_seconds_past_the_largest_one_held_is_read_as_the_largest() {
        assert_eq!(read("99999999999999999999999"), Some(RetryAfter::Delay(Duration::from_secs(u64::MAX))));
    }

    /// RFC 9110 section 5.6.7: a recipient accepts the IMF-fixdate and the
    /// two obsolete formats.
    #[test]
    fn a_date_in_any_of_the_three_formats_is_read_as_that_instant() {
        for text in ["Sun, 06 Nov 1994 08:49:37 GMT", "Sunday, 06-Nov-94 08:49:37 GMT", "Sun Nov  6 08:49:37 1994"] {
            assert_eq!(read(text), Some(RetryAfter::Date(example_date())), "{text}");
        }
    }

    /// `delay-seconds` is `1*DIGIT`, so a sign, a fraction or a unit makes the
    /// value neither form.
    #[test]
    fn a_value_that_is_neither_a_count_of_seconds_nor_a_date_is_refused() {
        for text in ["", "-5", "+5", "1.5", "5s", "soon", "Sun, 32 Nov 1994 08:49:37 GMT"] {
            assert_eq!(read(text), None, "{text:?}");
        }
        let opaque = HeaderValue::from_bytes(b"\xff").expect("an opaque field value");
        assert_eq!(decoded(&opaque), None);
    }

    #[test]
    fn a_delay_asks_for_itself_whenever_the_response_arrived() {
        let delay = RetryAfter::Delay(Duration::from_secs(7));
        assert_eq!(delay.wait_from(SystemTime::UNIX_EPOCH), Duration::from_secs(7));
        assert_eq!(delay.wait_from(SystemTime::now()), Duration::from_secs(7));
    }

    #[test]
    fn a_date_asks_for_the_time_until_it_and_a_date_in_the_past_for_none() {
        let date = RetryAfter::Date(example_date());
        let at = SystemTime::from(example_date());
        assert_eq!(date.wait_from(at - Duration::from_secs(30)), Duration::from_secs(30));
        assert_eq!(date.wait_from(at), Duration::ZERO);
        assert_eq!(date.wait_from(at + Duration::from_secs(5)), Duration::ZERO);
    }

    #[test]
    fn a_delay_is_written_in_whole_seconds_rounded_up() {
        assert_eq!(written(RetryAfter::Delay(Duration::from_secs(1))), Some(HeaderValue::from_static("1")));
        assert_eq!(written(RetryAfter::Delay(Duration::from_millis(1_500))), Some(HeaderValue::from_static("2")));
        assert_eq!(written(RetryAfter::Delay(Duration::ZERO)), Some(HeaderValue::from_static("0")));
    }

    #[test]
    fn a_date_is_written_as_an_imf_fixdate() {
        let date = read("Sunday, 06-Nov-94 08:49:37 GMT").expect("an obsolete RFC 850 date");
        assert_eq!(written(date), Some(HeaderValue::from_static("Sun, 06 Nov 1994 08:49:37 GMT")));
    }
}
