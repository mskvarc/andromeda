//! The most JSON values a request payload may hold.

use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;

/// The maximum number of JSON values one request payload may hold, carried as
/// a request extension.
///
/// The byte limit of [`RequestBodyLimit`](crate::request_body_limit::RequestBodyLimit)
/// does not bound what a payload costs once parsed: that cost follows the
/// number of values, and a body of short numbers packs millions of them into
/// a few megabytes. The HTTP stack installs this limit beside the byte limit on
/// every NGSI-LD request, and every extractor that parses a JSON payload counts
/// the payload's values against it before building anything from it. A request
/// carrying none (a router assembled without the stack, as in a unit test) is
/// not counted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequestJsonValueLimit(JsonValueLimit);

impl RequestJsonValueLimit {
    /// Construct a request JSON value limit.
    #[must_use]
    pub const fn new(limit: JsonValueLimit) -> RequestJsonValueLimit {
        RequestJsonValueLimit(limit)
    }

    /// The number of values a payload may hold.
    #[must_use]
    pub const fn limit(self) -> JsonValueLimit {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use crate::request_json_value_limit::RequestJsonValueLimit;
    use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
    use std::num::NonZeroUsize;

    #[test]
    fn a_limit_carries_the_value_count_it_was_built_with() {
        let limit = JsonValueLimit::new(NonZeroUsize::MIN.saturating_add(3_999_999));
        pretty_assertions::assert_eq!(RequestJsonValueLimit::new(limit).limit().get(), 4_000_000);
    }
}
