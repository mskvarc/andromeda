//! Where a context source's answer to a forwarded read continues (clauses
//! 5.5.9.1 and 6.3.10).
//!
//! Clause 5.5.9.1 makes the absence of a `next` pointer the only reliable sign
//! that a paginated answer has ended -- "the number of Elements retrieved per
//! page can be lower than L" -- and clause 6.3.10 makes that pointer "a
//! URI-reference that could be dereferenced by an NGSI-LD Client to retrieve
//! the next page". The broker walking a source's answer is such a client, so
//! the pointer is resolved against the URL the source answered, exactly as a
//! client would resolve it.
//!
//! A pointer is followed only to the origin of the registration's endpoint:
//! the registration is what the broker was told to contact (clause 4.3.6.1),
//! and a source cannot redirect the broker's forwarded request, with the
//! headers clause 6.3.19 attaches to it, to an arbitrary host.

use iri_rs::IriRefBuf;
use thiserror::Error;
use url::Url;

/// Where one page of a source's answer leaves the answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SourceContinuation {
    /// No `next` pointer: the answer ends on this page.
    Complete,
    /// The next page, resolved against the URL that answered this one; boxed
    /// because every forwarded answer carries a continuation and most carry
    /// none.
    Next(Box<Url>),
    /// A `next` pointer the broker does not follow.
    Unfollowable(UnfollowableNext),
}

/// Why a source's `next` pointer is not followed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub(crate) enum UnfollowableNext {
    /// The pointer does not resolve against the URL that answered.
    #[error("its next-page pointer does not resolve against the answering URL: {source}")]
    Unresolvable {
        /// Why it did not resolve.
        #[source]
        source: url::ParseError,
    },
    /// The pointer leaves the origin of the registration's endpoint.
    #[error("its next-page pointer `{target}` leaves the origin of the registered endpoint")]
    ForeignOrigin {
        /// The resolved pointer.
        target: Box<Url>,
    },
}

impl SourceContinuation {
    /// Where the page `answered` returned continues, given the `next` pointer
    /// the source sent and the registration `endpoint` it was forwarded to.
    #[must_use]
    pub(crate) fn of(next: Option<&IriRefBuf>, answered: &Url, endpoint: &Url) -> SourceContinuation {
        let Some(next) = next else {
            return SourceContinuation::Complete;
        };
        match answered.join(next.as_str()) {
            Ok(resolved) if resolved.origin() == endpoint.origin() => SourceContinuation::Next(Box::new(resolved)),
            Ok(resolved) => SourceContinuation::Unfollowable(UnfollowableNext::ForeignOrigin { target: Box::new(resolved) }),
            Err(source) => SourceContinuation::Unfollowable(UnfollowableNext::Unresolvable { source }),
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::source_continuation::{SourceContinuation, UnfollowableNext};
    use iri_rs::IriRefBuf;
    use url::Url;

    fn url(raw: &str) -> Url {
        Url::parse(raw).expect("valid URL")
    }

    fn reference(raw: &str) -> IriRefBuf {
        raw.parse().expect("valid IRI reference")
    }

    /// Clause 5.5.9.1: an answer without a `next` pointer has ended.
    #[test]
    fn an_answer_without_a_next_pointer_is_complete() {
        let answered = url("http://peer:1026/ngsi-ld/v1/entities?type=Vehicle&limit=1000");
        pretty_assertions::assert_eq!(SourceContinuation::of(None, &answered, &url("http://peer:1026")), SourceContinuation::Complete);
    }

    /// Clause 6.3.10: the pointer is a URI-reference, which a client resolves
    /// against the URL it dereferenced; the relative form Andromeda itself
    /// emits resolves onto the source's own path.
    #[test]
    fn a_relative_next_pointer_resolves_against_the_answering_url() {
        let answered = url("http://peer:1026/ngsi-ld/v1/entities?type=Vehicle&limit=1000");
        let next = reference("/ngsi-ld/v1/entities?type=Vehicle&limit=1000&offset=1000");
        pretty_assertions::assert_eq!(
            SourceContinuation::of(Some(&next), &answered, &url("http://peer:1026/")),
            SourceContinuation::Next(Box::new(url("http://peer:1026/ngsi-ld/v1/entities?type=Vehicle&limit=1000&offset=1000")))
        );
    }

    /// An absolute pointer on the registered origin is followed as written.
    #[test]
    fn an_absolute_next_pointer_on_the_registered_origin_is_followed() {
        let answered = url("http://peer:1026/ngsi-ld/v1/entities?type=Vehicle");
        let next = reference("http://peer:1026/ngsi-ld/v1/entities?type=Vehicle&cursor=abc");
        pretty_assertions::assert_eq!(
            SourceContinuation::of(Some(&next), &answered, &url("http://peer:1026/ngsi-ld/v1")),
            SourceContinuation::Next(Box::new(url("http://peer:1026/ngsi-ld/v1/entities?type=Vehicle&cursor=abc")))
        );
    }

    /// Clause 4.3.6.1: the broker contacts the registration it matched, so a
    /// pointer to another host, scheme or port is not followed.
    #[test]
    fn a_next_pointer_leaving_the_registered_origin_is_not_followed() {
        let answered = url("http://peer:1026/ngsi-ld/v1/entities?type=Vehicle");
        for foreign in [
            "http://elsewhere:1026/ngsi-ld/v1/entities?offset=10",
            "https://peer:1026/ngsi-ld/v1/entities?offset=10",
            "http://peer:9999/ngsi-ld/v1/entities?offset=10",
        ] {
            let continuation = SourceContinuation::of(Some(&reference(foreign)), &answered, &url("http://peer:1026"));
            assert!(
                matches!(continuation, SourceContinuation::Unfollowable(UnfollowableNext::ForeignOrigin { .. })),
                "{foreign}: {continuation:?}"
            );
        }
    }
}
