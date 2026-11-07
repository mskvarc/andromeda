//! How long one delivery attempt is given (Table 5.2.15-1 `timeout`).

use andromeda_ngsi_ld_lexicon::positive_milliseconds::PositiveMilliseconds;
use std::time::Duration;

/// The deadline one delivery attempt runs under.
///
/// Table 5.2.15-1 makes the endpoint's `timeout` the "maximum period of time in
/// milliseconds which may elapse before a notification is assumed to have
/// failed" and then says "the NGSI-LD system can override this value". The
/// override runs one way: `system` is the broker's own bound, so a subscriber
/// may shorten the wait but never lengthen it. Without that ceiling any
/// subscription could hold a delivery worker for the whole range
/// [`PositiveMilliseconds`] admits, which reaches millions of years.
///
/// An endpoint that named no timeout runs under the system bound alone.
///
/// The same arithmetic serves every binding whose "protocol always returns a
/// response", which is the condition the row attaches to the member: the HTTP
/// POST, and an MQTT publish at a quality of service the broker acknowledges.
#[must_use]
pub fn delivery_deadline(endpoint_timeout: Option<PositiveMilliseconds>, system: Duration) -> Duration {
    match endpoint_timeout {
        Some(timeout) => timeout.as_duration().min(system),
        None => system,
    }
}

#[cfg(test)]
mod tests {
    use crate::delivery_deadline::delivery_deadline;
    use andromeda_ngsi_ld_lexicon::positive_milliseconds::PositiveMilliseconds;
    use std::time::Duration;

    fn milliseconds(value: i64) -> PositiveMilliseconds {
        PositiveMilliseconds::try_from(value).expect("a positive millisecond count")
    }

    /// Table 5.2.15-1: the endpoint's own `timeout` bounds the attempt, and the
    /// system bound applies when the endpoint named none.
    #[test]
    fn an_endpoint_timeout_shortens_the_system_bound() {
        let system = Duration::from_secs(10);
        pretty_assertions::assert_eq!(delivery_deadline(Some(milliseconds(250)), system), Duration::from_millis(250));
        pretty_assertions::assert_eq!(delivery_deadline(None, system), system);
    }

    /// Table 5.2.15-1: "the NGSI-LD system can override this value", meaning the
    /// system overrides the endpoint, not the other way round. An endpoint
    /// that asks for longer than the system bound gets the system bound.
    #[test]
    fn an_endpoint_timeout_cannot_lengthen_the_system_bound() {
        let system = Duration::from_secs(10);
        pretty_assertions::assert_eq!(delivery_deadline(Some(milliseconds(60_000)), system), system);
    }

    /// The widest value the member admits is millions of years, which is
    /// exactly the hold on a delivery worker the ceiling exists to prevent.
    #[test]
    fn the_widest_endpoint_timeout_is_still_held_to_the_system_bound() {
        let system = Duration::from_secs(10);
        pretty_assertions::assert_eq!(delivery_deadline(Some(milliseconds(i64::MAX)), system), system);
    }
}
