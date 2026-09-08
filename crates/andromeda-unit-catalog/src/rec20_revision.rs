//! The revision of UN/CEFACT Recommendation 20 a registration belongs to.

use derive_more::Display;
use thiserror::Error;

/// A revision number of UN/CEFACT Recommendation 20.
///
/// Revisions are whole, positive and monotonically increasing: revision 9 is
/// the one NGSI-LD v1.9.1 cites in its normative reference list, revision 17
/// (2021) is the one this catalog is generated from. The number is stored
/// alongside every registration so a code that a later revision withdraws stays
/// interpretable rather than becoming an identifier with no provenance.
#[derive(Clone, Copy, Debug, Display, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[display("{_0}")]
pub struct Rec20Revision(i16);

impl Rec20Revision {
    /// Return the revision number.
    #[must_use]
    pub const fn get(self) -> i16 {
        self.0
    }
}

impl TryFrom<i16> for Rec20Revision {
    type Error = Rec20RevisionOutOfRange;

    fn try_from(revision: i16) -> Result<Rec20Revision, Rec20RevisionOutOfRange> {
        if revision > 0 {
            Ok(Rec20Revision(revision))
        } else {
            Err(Rec20RevisionOutOfRange)
        }
    }
}

/// A revision number was not a positive whole number.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("a Recommendation 20 revision is a positive whole number")]
pub struct Rec20RevisionOutOfRange;

/// The Recommendation 20 revision this catalog release is generated from.
///
/// Revision 17 is the profile the generating artifact publishes. NGSI-LD
/// v1.9.1 reference [15] pins revision 9, which revision 17 is a superset of:
/// every code revision 9 defines is still assigned here, so a request written
/// against the specification's own revision is accepted unchanged, while codes
/// added since are accepted as well.
pub const REC20_REVISION: Rec20Revision = Rec20Revision(17);

#[cfg(test)]
mod tests {
    use crate::rec20_revision::{REC20_REVISION, Rec20Revision, Rec20RevisionOutOfRange};

    #[test]
    fn the_catalog_release_records_the_revision_of_its_source_artifact() {
        assert_eq!(REC20_REVISION.get(), 17);
    }

    #[test]
    fn a_revision_number_round_trips_through_its_stored_form() {
        let revision = Rec20Revision::try_from(9_i16);
        assert_eq!(revision.map(Rec20Revision::get), Ok(9));
    }

    #[test]
    fn a_zero_or_negative_revision_number_is_refused() {
        assert_eq!(Rec20Revision::try_from(0_i16), Err(Rec20RevisionOutOfRange));
        assert_eq!(Rec20Revision::try_from(-1_i16), Err(Rec20RevisionOutOfRange));
    }

    #[test]
    fn revisions_order_by_their_number() {
        let ninth = Rec20Revision::try_from(9_i16);
        assert!(ninth.is_ok_and(|earlier| earlier < REC20_REVISION));
    }
}
