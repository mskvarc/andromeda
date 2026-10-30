//! Which of the registrations matching a forwarded read it contacts.
//!
//! A read is forwarded to every registration that matches it (clause 5.12),
//! but two kinds of read narrow that set by registration: the re-assembly of a
//! notified entity skips the source that sent the notification, which already
//! supplied its part (clause 5.8.6), and a request repeated to follow up one
//! source's answer goes to that source alone.

use andromeda_ngsi_ld_lexicon::identifier::Identifier;

/// Which of the matching registrations a forwarded read contacts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SourceReach<'a> {
    /// Every matching registration.
    #[default]
    Every,
    /// Every matching registration except this one.
    AllBut(&'a Identifier),
    /// This registration alone, when it matches.
    Only(&'a Identifier),
}

impl SourceReach<'_> {
    /// Whether the registration `id` is contacted.
    #[must_use]
    pub fn admits(self, id: &Identifier) -> bool {
        match self {
            SourceReach::Every => true,
            SourceReach::AllBut(excluded) => excluded != id,
            SourceReach::Only(only) => only == id,
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::source_reach::SourceReach;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;

    fn registration(raw: &str) -> Identifier {
        raw.parse().expect("a valid identifier")
    }

    #[test]
    fn each_reach_admits_the_registrations_it_names() {
        let first = registration("urn:ngsi-ld:csr:1");
        let second = registration("urn:ngsi-ld:csr:2");
        assert!(SourceReach::Every.admits(&first));
        assert!(!SourceReach::AllBut(&first).admits(&first));
        assert!(SourceReach::AllBut(&first).admits(&second));
        assert!(SourceReach::Only(&first).admits(&first));
        assert!(!SourceReach::Only(&first).admits(&second));
    }
}
