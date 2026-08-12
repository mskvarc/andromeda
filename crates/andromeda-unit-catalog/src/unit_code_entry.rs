//! One registered unit code and the revision span it is valid for.

use crate::{rec20_revision::Rec20Revision, unit_code_id::UnitCodeId, unit_code_token::UnitCodeToken};
use cefact_units::UnitCode;

/// One row of the append-only unit-code registry.
///
/// The revision span is provenance, not a filter: a registration whose
/// `retired_revision` is set is still decodable, because rows written while the
/// code was current keep pointing at it. Withdrawal says only that a client may
/// no longer introduce the code, which is a request-validation decision, not a
/// storage one.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct UnitCodeEntry {
    /// The identity persisted in place of the token.
    id: UnitCodeId,
    /// The common code as written on the wire.
    token: UnitCodeToken,
    /// The revision this code was first registered from.
    first_revision: Rec20Revision,
    /// The revision that withdrew this code, when one has.
    retired_revision: Option<Rec20Revision>,
}

impl UnitCodeEntry {
    /// Register `token` under `id`, first seen in `first_revision` and
    /// withdrawn in `retired_revision`.
    pub const fn new(id: UnitCodeId, token: UnitCodeToken, first_revision: Rec20Revision, retired_revision: Option<Rec20Revision>) -> UnitCodeEntry {
        UnitCodeEntry {
            id,
            token,
            first_revision,
            retired_revision,
        }
    }

    /// Return the persisted identity.
    pub const fn id(&self) -> UnitCodeId {
        self.id
    }

    /// Return the common code as written on the wire.
    pub const fn token(&self) -> UnitCodeToken {
        self.token
    }

    /// Return the revision this code was first registered from.
    pub const fn first_revision(&self) -> Rec20Revision {
        self.first_revision
    }

    /// Return the revision that withdrew this code, when one has.
    pub const fn retired_revision(&self) -> Option<Rec20Revision> {
        self.retired_revision
    }

    /// Return the typed code, when the linked Recommendation 20 artifact still
    /// defines one for this token.
    pub fn unit_code(&self) -> Option<UnitCode> {
        UnitCode::from_code(self.token.as_str())
    }
}

#[cfg(test)]
mod tests {
    use crate::{rec20_revision::Rec20Revision, unit_code_entry::UnitCodeEntry, unit_code_id::UnitCodeId, unit_code_token::UnitCodeToken};

    /// Build one registration for a code the linked artifact defines.
    fn kilogram() -> Option<UnitCodeEntry> {
        let token = UnitCodeToken::try_from("KGM").ok()?;
        let revision = Rec20Revision::try_from(17_i16).ok()?;
        Some(UnitCodeEntry::new(UnitCodeId::first(), token, revision, None))
    }

    #[test]
    fn a_current_registration_resolves_to_a_typed_code() {
        let resolved = kilogram().and_then(|entry| entry.unit_code());
        assert_eq!(resolved.map(|code| code.as_str()), Some("KGM"));
    }

    #[test]
    fn a_registration_reports_the_revision_it_was_first_seen_in() {
        assert_eq!(kilogram().map(|entry| entry.first_revision().get()), Some(17));
        assert_eq!(kilogram().map(|entry| entry.retired_revision()), Some(None));
    }

    #[test]
    fn a_token_the_linked_artifact_no_longer_defines_stays_readable() {
        let withdrawn = UnitCodeToken::try_from("ZZZ").ok().and_then(|token| {
            let first = Rec20Revision::try_from(9_i16).ok()?;
            let retired = Rec20Revision::try_from(17_i16).ok()?;
            Some(UnitCodeEntry::new(UnitCodeId::first(), token, first, Some(retired)))
        });
        assert_eq!(withdrawn.map(|entry| entry.token().as_str()), Some("ZZZ"));
        assert_eq!(withdrawn.and_then(|entry| entry.unit_code()), None);
    }
}
