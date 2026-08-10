//! API-wide request-body admission limit.

/// Maximum accepted request body size in bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequestBodyLimit(usize);

impl RequestBodyLimit {
    /// Construct a request-body limit.
    pub const fn new(bytes: usize) -> RequestBodyLimit {
        RequestBodyLimit(bytes)
    }

    /// Return the configured byte count.
    pub const fn bytes(self) -> usize {
        self.0
    }
}
