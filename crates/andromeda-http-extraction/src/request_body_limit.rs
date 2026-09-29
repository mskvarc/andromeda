//! The largest request payload body the broker accepts.

/// The maximum request payload body size in bytes, carried as a request
/// extension.
///
/// The HTTP stack installs it on every NGSI-LD request beside the limit it
/// enforces, so [`buffer_payload`](crate::request_payload::buffer_payload) can
/// tell the client how large a body the broker accepts when it refuses one as
/// too large (clause 6.3.2, "Request Entity too large"). A request carrying
/// none -- a router assembled without the stack, as in a unit test -- is
/// refused without the figure.
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

#[cfg(test)]
mod tests {
    use crate::request_body_limit::RequestBodyLimit;

    #[test]
    fn a_limit_carries_the_byte_count_it_was_built_with() {
        pretty_assertions::assert_eq!(RequestBodyLimit::new(67_108_864).bytes(), 67_108_864);
    }
}
