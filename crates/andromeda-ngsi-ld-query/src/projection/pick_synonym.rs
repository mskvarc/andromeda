//! The members a request names the deprecated synonym for `pick` by.
//!
//! The synonym is "A synonym for pick, except that id, type, scope are not allowed. Deprecated" wherever it appears:
//! the `attrs` parameter of a query or retrieval (Tables 6.4.3.2-1 and 6.5.3.1-1) and the `attributes` member of a
//! Subscription's notification (Table 5.2.14.1-1). A refusal names the member the request wrote.

use strum::Display;

/// Where a request wrote the deprecated synonym for `pick`.
#[derive(Clone, Copy, Debug, Display, Eq, PartialEq)]
pub enum PickSynonym {
    /// The `attrs` query parameter.
    #[strum(serialize = "attrs")]
    AttrsParameter,
    /// The `attributes` member of a Subscription's `notification`.
    #[strum(serialize = "notification.attributes")]
    NotificationAttributes,
}

#[cfg(test)]
mod tests {
    use crate::projection::pick_synonym::PickSynonym;

    #[test]
    fn each_synonym_is_named_as_the_request_writes_it() {
        pretty_assertions::assert_eq!(PickSynonym::AttrsParameter.to_string(), "attrs");
        pretty_assertions::assert_eq!(PickSynonym::NotificationAttributes.to_string(), "notification.attributes");
    }
}
