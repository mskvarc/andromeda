//! The members of a `Subscription` payload, by the names clause 5.2.12 gives them.

use strum::{Display, IntoStaticStr};

/// A typed member of a `Subscription` payload (Table 5.2.12-1).
///
/// Spelt as the specification spells it, both where a payload member is looked
/// up and where a refusal names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, IntoStaticStr)]
#[strum(serialize_all = "camelCase")]
pub enum SubscriptionMember {
    /// `entities`.
    Entities,
    /// `watchedAttributes`.
    WatchedAttributes,
    /// `notificationTrigger`.
    NotificationTrigger,
    /// `q`.
    Q,
    /// `scopeQ`.
    ScopeQ,
    /// `csf`.
    Csf,
    /// `datasetId`.
    DatasetId,
    /// `notification`.
    Notification,
    /// `expandValues`.
    ExpandValues,
    /// `jsonKeys`.
    JsonKeys,
    /// `lang`.
    Lang,
    /// `jsonldContext`.
    JsonldContext,
}

impl SubscriptionMember {
    /// The member name as a payload spells it.
    #[must_use]
    pub fn key(self) -> &'static str {
        self.into()
    }
}

#[cfg(test)]
mod tests {
    use crate::subscription_member::SubscriptionMember;
    use pretty_assertions::assert_eq;

    #[test]
    fn members_are_spelt_as_the_specification_spells_them() {
        assert_eq!(SubscriptionMember::WatchedAttributes.key(), "watchedAttributes");
        assert_eq!(SubscriptionMember::Q.key(), "q");
        assert_eq!(SubscriptionMember::ScopeQ.to_string(), "scopeQ");
    }
}
