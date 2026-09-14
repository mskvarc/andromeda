//! How long a downloaded `@context` may be served before it is fetched again.
//!
//! Clause 6.3.16 requires compliance with the `Expires` header field (IETF
//! RFC 7234 section 5.3) and with the `max-age` and `s-maxage` response
//! directives of the `Cache-Control` header field (RFC 7234 section 5.2.2)
//! carried by a downloaded `@context`, so that the entries clause 5.13.1 flags
//! `"Cached"` are invalidated according to them. RFC 7234 section 4.2.1 fixes
//! the precedence a cache applies: a shared cache takes `s-maxage` first, then
//! `max-age`, then `Expires` measured from the response `Date`. A broker caches
//! on behalf of every client it serves, so it is a shared cache and `s-maxage`
//! applies to it.
//!
//! Clause 6.3.16 also recommends assigning a heuristic expiration time when the
//! origin server states none. The heuristic here is the deployment's configured
//! `jsonld.cache_ttl_secs`, which is why that setting is a fallback rather
//! than the lifetime of every entry.
//!
//! The heuristic applies only where the origin server genuinely stated nothing.
//! A field that is present but cannot be read in full, and a date that is
//! present but will not parse, both state something — and neither states that
//! the document may be kept for an hour.

use headers::{CacheControl, Date, Expires, HeaderMapExt};
use reqwest::header::{CACHE_CONTROL, EXPIRES, HeaderMap, HeaderValue};
use std::time::{Duration, SystemTime};

/// A positive span for which a stored `@context` stays fresh.
///
/// Zero is deliberately not representable: a document that is stale the instant
/// it arrives is not cacheable at all, which [`ContextFreshness::Uncacheable`]
/// records instead. Keeping the two apart means no cache tier has to decide
/// what a zero lifetime ought to mean.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CacheLifetime(Duration);

impl CacheLifetime {
    /// The lifetime assigned when neither the response nor the deployment
    /// states one.
    ///
    /// One hour, matching the `jsonld.cache_ttl_secs` default, so a builder
    /// driven without a configuration behaves as a configured deployment does.
    pub const DEFAULT: CacheLifetime = CacheLifetime(Duration::from_secs(3600));

    /// The lifetime `duration` grants, or `None` when it grants none.
    #[must_use]
    pub const fn new(duration: Duration) -> Option<CacheLifetime> {
        if duration.is_zero() { None } else { Some(CacheLifetime(duration)) }
    }

    /// The lifetime `seconds` grants, or `None` when it grants none.
    #[must_use]
    pub const fn from_seconds(seconds: u64) -> Option<CacheLifetime> {
        CacheLifetime::new(Duration::from_secs(seconds))
    }

    /// How long the entry stays fresh.
    #[must_use]
    pub const fn as_duration(self) -> Duration {
        self.0
    }

    /// How long the entry stays fresh, as a number of seconds.
    ///
    /// The durable store dates a row `CURRENT_TIMESTAMP + make_interval(secs =>
    /// …)`, whose argument is a number rather than a duration. Fractional
    /// seconds are kept: an `Expires` measured against the local clock rarely
    /// lands on a whole second, and truncating one away would shorten the very
    /// lifetime the origin server granted.
    #[must_use]
    pub const fn as_seconds(self) -> f64 {
        self.0.as_secs_f64()
    }
}

/// What a response permits a cache to do with the `@context` it carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextFreshness {
    /// Storing the document is forbidden, or it was already stale on arrival.
    Uncacheable,
    /// The document may be stored and served for this long.
    Fresh(CacheLifetime),
}

/// How much of a stored `@context`'s lifetime is still ahead of it.
///
/// A durable cache hands this back with every hit, so the in-process tier is
/// held to the span the origin server granted instead of being re-bounded to
/// the heuristic (clause 6.3.16). A lapsed entry is deliberately not
/// representable: clause 5.13.1 forbids serving one, so it never becomes a hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResidualLifetime {
    /// The entry records no expiry, so no origin server can make it stale.
    Unbounded,
    /// The entry may be kept for this much longer.
    Bounded(CacheLifetime),
}

/// The freshness a response grants the `@context` it carries.
///
/// `default` is the heuristic expiration time clause 6.3.16 recommends
/// assigning when the origin server states no explicit one; it is used only
/// when the response states neither a `Cache-Control` freshness directive nor
/// an `Expires` field.
#[must_use]
pub fn response_freshness(headers: &HeaderMap, default: CacheLifetime) -> ContextFreshness {
    match cache_control_freshness(headers) {
        Some(freshness) => freshness,
        None => expires_freshness(headers, default),
    }
}

/// What the `Cache-Control` field settles on its own.
///
/// `None` means the field left the decision to `Expires`: RFC 7234 section
/// 4.2.1 consults `Expires` only once `s-maxage` and `max-age` are both absent.
fn cache_control_freshness(headers: &HeaderMap) -> Option<ContextFreshness> {
    match cache_control(headers) {
        ResponseCacheControl::Silent => None,
        // The field was sent and could not be read in full, so nothing in it
        // can be shown to permit storage. RFC 7234 section 3 lets a cache store
        // only a response it was permitted to store, and a `no-store` origin
        // server whose field is unreadable must not be cached for an hour
        // because the bytes were awkward.
        ResponseCacheControl::Unreadable => Some(ContextFreshness::Uncacheable),
        ResponseCacheControl::Directives(control) => {
            // Neither directive leaves a lifetime to compute: `no-store`
            // forbids storage outright and `no-cache` forbids serving a stored
            // copy without revalidating it, which this resolver does not do.
            if control.no_store() || control.no_cache() {
                return Some(ContextFreshness::Uncacheable);
            }
            // RFC 7234 section 5.2.2.9: `s-maxage` overrides `max-age` for a
            // shared cache, which a broker's `@context` store is — in both
            // directions, whichever of the two spans is the longer.
            control.s_max_age().or_else(|| control.max_age()).map(granted)
        }
    }
}

/// What the `Expires` field grants, or the heuristic when there is no field.
///
/// RFC 7234 section 5.3 requires a cache recipient to "interpret invalid date
/// formats, especially the value `0`, as representing a time in the past (i.e.
/// 'already expired')", so a field that will not parse is a stale document
/// rather than a silent one — the distinction clause 6.3.16 rests on, since
/// only a silent origin server earns the heuristic expiration time.
fn expires_freshness(headers: &HeaderMap, default: CacheLifetime) -> ContextFreshness {
    if !headers.contains_key(EXPIRES) {
        return ContextFreshness::Fresh(default);
    }
    match headers.typed_get::<Expires>() {
        Some(expires) => granted(remaining_until(headers, expires)),
        None => ContextFreshness::Uncacheable,
    }
}

/// What a response's `Cache-Control` field says, once every value that can be
/// read has been read.
enum ResponseCacheControl {
    /// The field is absent, or names no directive this cache recognises, so it
    /// states nothing about freshness.
    Silent,
    /// Every value was readable and at least one recognised directive was
    /// parsed from them.
    Directives(CacheControl),
    /// The field is present and at least one of its values is not readable
    /// text, so what the origin server permitted cannot be established.
    Unreadable,
}

/// The response's `Cache-Control` field, read case-insensitively.
///
/// IETF RFC 9111 section 5.2 makes directive names case-insensitive, while
/// `headers::CacheControl` recognises them only in lower case, so the field is
/// case-folded before it is parsed — an origin server answering `No-Store` must
/// be obeyed as surely as one answering `no-store`. Every directive this module
/// reads takes a numeric argument or none at all, so folding the whole field
/// value cannot alter one.
///
/// A value that is not readable text is skipped rather than taken to discard
/// the field, so a readable `no-store` beside an unreadable value still binds
/// the cache; that the field was incomplete is reported separately.
fn cache_control(headers: &HeaderMap) -> ResponseCacheControl {
    let mut folded = HeaderMap::with_capacity(1);
    let mut supplied = 0_usize;
    for value in headers.get_all(CACHE_CONTROL) {
        supplied += 1;
        if let Some(readable) = case_folded(value) {
            folded.append(CACHE_CONTROL, readable);
        }
    }
    if supplied == 0 {
        return ResponseCacheControl::Silent;
    }
    if folded.len() < supplied {
        return ResponseCacheControl::Unreadable;
    }
    folded
        .typed_get::<CacheControl>()
        .map_or(ResponseCacheControl::Silent, ResponseCacheControl::Directives)
}

/// One `Cache-Control` value in lower case, or `None` when it is not readable
/// text.
fn case_folded(value: &HeaderValue) -> Option<HeaderValue> {
    let lowercase = value.to_str().ok()?.to_ascii_lowercase();
    HeaderValue::from_str(&lowercase).ok()
}

/// How much of the span an `Expires` field names is still ahead.
///
/// RFC 7234 section 4.2.1 measures it from the response `Date`; a response that
/// omits `Date` is measured from the moment it arrived instead. An `Expires`
/// that has already passed leaves nothing, which reads as a zero span rather
/// than as a failure — a stale document is a cache decision, not an error.
fn remaining_until(headers: &HeaderMap, expires: Expires) -> Duration {
    let issued = headers.typed_get::<Date>().map_or_else(SystemTime::now, SystemTime::from);
    SystemTime::from(expires).duration_since(issued).unwrap_or(Duration::ZERO)
}

/// A positive span is a lifetime; a zero one leaves the document uncacheable.
const fn granted(span: Duration) -> ContextFreshness {
    match CacheLifetime::new(span) {
        Some(lifetime) => ContextFreshness::Fresh(lifetime),
        None => ContextFreshness::Uncacheable,
    }
}

#[cfg(test)]
mod tests {
    use crate::loader::cache_lifetime::{CacheLifetime, ContextFreshness, ResidualLifetime, response_freshness};
    use headers::{Date, Expires, HeaderMapExt};
    use reqwest::header::{CACHE_CONTROL, EXPIRES, HeaderMap, HeaderValue};
    use std::time::{Duration, SystemTime};

    /// The heuristic lifetime the tests fall back to, distinct from every
    /// lifetime a response grants so a fallback cannot be mistaken for a
    /// directive.
    fn configured_default() -> CacheLifetime {
        CacheLifetime::from_seconds(3600).expect("an hour is a positive lifetime")
    }

    fn with_cache_control(value: &'static str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(CACHE_CONTROL, HeaderValue::from_static(value));
        headers
    }

    /// A header value carrying a byte that is not readable text, which an
    /// origin server is free to send and `HeaderValue::to_str` refuses.
    fn unreadable_value() -> HeaderValue {
        HeaderValue::from_bytes(b"private, note=\xff\xfe").expect("obs-text bytes are a valid header value")
    }

    fn fresh_for(freshness: ContextFreshness) -> Option<Duration> {
        match freshness {
            ContextFreshness::Uncacheable => None,
            ContextFreshness::Fresh(lifetime) => Some(lifetime.as_duration()),
        }
    }

    /// Clause 6.3.16: a `max-age` response directive is the entry's lifetime,
    /// not merely a permission to store it for the configured time.
    #[test]
    fn a_positive_max_age_becomes_the_entry_lifetime() {
        let freshness = response_freshness(&with_cache_control("public, max-age=60"), configured_default());

        pretty_assertions::assert_eq!(fresh_for(freshness), Some(Duration::from_secs(60)));
    }

    /// RFC 7234 section 5.2.2.9, referenced by clause 6.3.16: `s-maxage`
    /// overrides `max-age` for the shared cache a broker keeps.
    #[test]
    fn s_maxage_overrides_max_age_for_a_shared_cache() {
        let freshness = response_freshness(&with_cache_control("max-age=600, s-maxage=30"), configured_default());

        pretty_assertions::assert_eq!(fresh_for(freshness), Some(Duration::from_secs(30)));
    }

    /// RFC 7234 section 5.2.2.9 makes `s-maxage` override `max-age` outright,
    /// not merely tighten it: a shared cache that took the smaller of the two
    /// would honour `max-age=1` here, and the origin server said the shared
    /// copy lives an hour.
    #[test]
    fn s_maxage_overrides_max_age_even_when_it_is_the_longer_span() {
        let freshness = response_freshness(&with_cache_control("max-age=1, s-maxage=3600"), configured_default());

        pretty_assertions::assert_eq!(fresh_for(freshness), Some(Duration::from_secs(3600)));
    }

    /// Clause 6.3.16: the `Expires` header field (RFC 7234 section 5.3) is
    /// honoured on its own, measured from the response `Date`.
    #[test]
    fn expires_alone_is_measured_from_the_response_date() {
        let issued = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let mut headers = HeaderMap::new();
        headers.typed_insert(Date::from(issued));
        headers.typed_insert(Expires::from(issued + Duration::from_secs(120)));

        let freshness = response_freshness(&headers, configured_default());

        pretty_assertions::assert_eq!(fresh_for(freshness), Some(Duration::from_secs(120)));
    }

    /// RFC 7234 section 4.2.1, which clause 6.3.16 binds: a `max-age` directive
    /// is consulted before `Expires`, so a response naming both is governed by
    /// the directive however far out the date is.
    #[test]
    fn max_age_overrides_a_conflicting_expires() {
        let issued = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let mut headers = with_cache_control("public, max-age=30");
        headers.typed_insert(Date::from(issued));
        headers.typed_insert(Expires::from(issued + Duration::from_hours(24)));

        let freshness = response_freshness(&headers, configured_default());

        pretty_assertions::assert_eq!(fresh_for(freshness), Some(Duration::from_secs(30)));
    }

    /// A `Cache-Control` field that names no freshness directive does not
    /// suppress the `Expires` field beside it.
    #[test]
    fn expires_still_applies_when_cache_control_names_no_freshness_directive() {
        let issued = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let mut headers = with_cache_control("public");
        headers.typed_insert(Date::from(issued));
        headers.typed_insert(Expires::from(issued + Duration::from_secs(45)));

        let freshness = response_freshness(&headers, configured_default());

        pretty_assertions::assert_eq!(fresh_for(freshness), Some(Duration::from_secs(45)));
    }

    /// An `Expires` in the past leaves no lifetime, so the document is stale on
    /// arrival and is never stored.
    #[test]
    fn an_expires_already_in_the_past_leaves_the_document_uncacheable() {
        let issued = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let mut headers = HeaderMap::new();
        headers.typed_insert(Date::from(issued));
        headers.typed_insert(Expires::from(issued - Duration::from_secs(1)));

        pretty_assertions::assert_eq!(response_freshness(&headers, configured_default()), ContextFreshness::Uncacheable);
    }

    /// RFC 7234 section 5.3, which clause 6.3.16 binds: "A cache recipient MUST
    /// interpret invalid date formats, especially the value `0`, as
    /// representing a time in the past (i.e. 'already expired')." A present but
    /// unparseable `Expires` is therefore a stale document, never an origin
    /// server that stated nothing and earned the heuristic lifetime.
    #[test]
    fn an_expires_that_will_not_parse_is_already_expired() {
        for stated in ["0", "-1", "not a date", ""] {
            let mut headers = HeaderMap::new();
            headers.insert(EXPIRES, HeaderValue::from_str(stated).expect("a header value"));

            pretty_assertions::assert_eq!(
                response_freshness(&headers, configured_default()),
                ContextFreshness::Uncacheable,
                "Expires: {stated} names a time in the past"
            );
        }
    }

    /// Clause 6.3.16: an implementation should assign a heuristic expiration
    /// time when the origin server specifies none.
    #[test]
    fn a_response_without_directives_falls_back_to_the_configured_lifetime() {
        let freshness = response_freshness(&HeaderMap::new(), configured_default());

        pretty_assertions::assert_eq!(fresh_for(freshness), Some(Duration::from_secs(3600)));
    }

    /// IETF RFC 9111 section 5.2: directive names are case-insensitive, so a
    /// mixed-case `No-Store` binds the cache exactly as a lower-case one does.
    #[test]
    fn directive_names_are_read_case_insensitively() {
        pretty_assertions::assert_eq!(
            response_freshness(&with_cache_control("No-Store"), configured_default()),
            ContextFreshness::Uncacheable
        );
        pretty_assertions::assert_eq!(
            fresh_for(response_freshness(&with_cache_control("Public, S-MaxAge=30"), configured_default())),
            Some(Duration::from_secs(30))
        );
    }

    #[test]
    fn directives_that_forbid_reuse_leave_the_document_uncacheable() {
        for value in ["no-store", "NO-CACHE", "public, max-age=0", "s-maxage=0, max-age=600"] {
            pretty_assertions::assert_eq!(
                response_freshness(&with_cache_control(value), configured_default()),
                ContextFreshness::Uncacheable,
                "{value} must leave the document uncacheable"
            );
        }
    }

    /// Clause 6.3.16 binds the whole `Cache-Control` field, not the values of
    /// it that happen to be readable. A `no-store` beside a value carrying
    /// bytes that are not text used to be discarded along with it, and the
    /// response then earned the heuristic lifetime — caching, for an hour, a
    /// document the origin server forbade storing at all.
    #[test]
    fn an_unreadable_value_does_not_discard_a_readable_no_store_beside_it() {
        let mut headers = HeaderMap::new();
        headers.append(CACHE_CONTROL, HeaderValue::from_static("no-store"));
        headers.append(CACHE_CONTROL, unreadable_value());

        pretty_assertions::assert_eq!(response_freshness(&headers, configured_default()), ContextFreshness::Uncacheable);
    }

    /// A `Cache-Control` field that was sent but cannot be read in full states
    /// something; it just cannot be shown to permit storage. RFC 7234 section 3
    /// stores only what a cache was permitted to store, so the document is not
    /// kept — the heuristic of clause 6.3.16 is for an origin server that
    /// stated nothing.
    #[test]
    fn a_cache_control_that_cannot_be_read_forbids_storing_the_document() {
        let mut headers = HeaderMap::new();
        headers.insert(CACHE_CONTROL, unreadable_value());

        pretty_assertions::assert_eq!(response_freshness(&headers, configured_default()), ContextFreshness::Uncacheable);
    }

    /// A readable field naming only extension directives states nothing this
    /// cache understands, which is not the same as stating something it cannot
    /// read: the `Expires` beside it still governs.
    #[test]
    fn a_readable_field_of_unrecognised_directives_leaves_expires_in_charge() {
        let issued = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let mut headers = with_cache_control("x-vendor-directive");
        headers.typed_insert(Date::from(issued));
        headers.typed_insert(Expires::from(issued + Duration::from_secs(90)));

        pretty_assertions::assert_eq!(fresh_for(response_freshness(&headers, configured_default())), Some(Duration::from_secs(90)));
    }

    #[test]
    fn a_zero_span_is_not_a_lifetime() {
        assert!(CacheLifetime::from_seconds(0).is_none());
        assert!(CacheLifetime::new(Duration::ZERO).is_none());
    }

    #[test]
    fn a_lifetime_reports_its_span_in_both_forms() {
        let lifetime = CacheLifetime::from_seconds(90).expect("ninety seconds is a positive lifetime");

        pretty_assertions::assert_eq!(lifetime.as_duration(), Duration::from_secs(90));
        pretty_assertions::assert_eq!(Duration::from_secs_f64(lifetime.as_seconds()), Duration::from_secs(90));
    }

    /// Clause 5.13.1 forbids serving a lapsed `"Cached"` entry, so a residual
    /// lifetime is either a positive span or the absence of any expiry.
    #[test]
    fn a_residual_lifetime_is_a_positive_span_or_no_expiry_at_all() {
        let lifetime = CacheLifetime::from_seconds(30).expect("thirty seconds is a positive lifetime");

        pretty_assertions::assert_eq!(ResidualLifetime::Bounded(lifetime), ResidualLifetime::Bounded(lifetime));
        assert_ne!(ResidualLifetime::Unbounded, ResidualLifetime::Bounded(lifetime));
    }
}
