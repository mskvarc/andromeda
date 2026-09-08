//! API-wide request-body admission limit.

/// Maximum accepted request body size in bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequestBodyLimit(usize);

impl RequestBodyLimit {
    /// Construct a request-body limit.
    #[must_use]
    pub const fn new(bytes: usize) -> RequestBodyLimit {
        RequestBodyLimit(bytes)
    }

    /// Return the configured byte count.
    #[must_use]
    pub const fn bytes(self) -> usize {
        self.0
    }
}
