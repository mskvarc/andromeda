//! Whether a context source's error answer to a forwarded request is its
//! verdict on the request as sent.
//!
//! A source that refuses a request as sent refuses the same request every time
//! it is sent. A request timeout (RFC 9110 section 15.5.9), a rate limit (RFC
//! 6585 section 4), an internal error, a bad gateway, an unavailable service
//! and a gateway timeout (RFC 9110 sections 15.6.1 and 15.6.3 to 15.6.5)
//! depend on the moment the request came instead, so they say nothing of how
//! the same request is answered at another moment.

/// Whether an error answer is the source's verdict on the request as sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnswerVerdict {
    /// The source refused the request as sent, as it refuses the same request
    /// every time it is sent.
    Refused,
    /// The answer depends on the moment the request came, so the same request
    /// may be answered otherwise at another moment.
    Withheld,
}
