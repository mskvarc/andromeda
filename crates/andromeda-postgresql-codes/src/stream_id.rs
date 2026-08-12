//! The identity of one temporal attribute stream.

use derive_more::{Display, From, Into};
use uuid::Uuid;

/// The identity of one temporal attribute stream.
///
/// A stream is the stable grain of temporal history — one entity, one attribute
/// name, one dataset — and its identity is what every history row segments by.
/// Newly created streams are named with a version 7 UUID so that identities
/// minted close together sort close together and cluster on write; identities
/// minted by earlier releases as version 4 are equally valid and are never
/// rewritten, because rewriting them would detach the history that already
/// names them.
///
/// Every UUID is therefore a well-formed stream identity, which is why the
/// conversion from [`Uuid`] is total.
#[derive(Clone, Copy, Debug, Display, Eq, From, Hash, Into, Ord, PartialEq, PartialOrd)]
#[display("{_0}")]
pub struct StreamId(Uuid);

impl StreamId {
    /// Mint the identity of a stream being created now.
    pub fn generate() -> StreamId {
        StreamId(Uuid::now_v7())
    }

    /// Return the stored representation.
    pub const fn get(self) -> Uuid {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use crate::stream_id::StreamId;
    use uuid::Uuid;

    #[test]
    fn a_stored_identity_round_trips() {
        let stored = Uuid::from_u128(0x0194_2f6c_7f00_7000_8000_0000_0000_0001);
        assert_eq!(StreamId::from(stored).get(), stored);
        assert_eq!(Uuid::from(StreamId::from(stored)), stored);
    }

    #[test]
    fn a_minted_identity_is_a_version_seven_uuid() {
        assert_eq!(StreamId::generate().get().get_version_num(), 7);
    }

    #[test]
    fn two_minted_identities_differ() {
        assert_ne!(StreamId::generate(), StreamId::generate());
    }

    #[test]
    fn an_identity_minted_by_an_earlier_release_is_accepted_unchanged() {
        let version_four = Uuid::from_u128(0x3fa8_5f64_5717_4562_b3fc_2c96_3f66_afa6);
        assert_eq!(version_four.get_version_num(), 4);
        assert_eq!(StreamId::from(version_four).get(), version_four);
    }

    #[test]
    fn an_identity_renders_as_the_canonical_uuid_it_stores() {
        let stored = Uuid::from_u128(0x3fa8_5f64_5717_4562_b3fc_2c96_3f66_afa6);
        assert_eq!(StreamId::from(stored).to_string(), "3fa85f64-5717-4562-b3fc-2c963f66afa6");
    }
}
