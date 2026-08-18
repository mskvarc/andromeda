//! The narrow key a temporal attribute stream is referenced by.

use derive_more::Display;
use thiserror::Error;

/// The surrogate key of one temporal attribute stream.
///
/// A stream is the stable grain of temporal history — one entity, one attribute
/// name, one dataset — and its key is what every history row segments by and
/// what the stream-led history indexes lead with. Because that key repeats on
/// every history row and in every one of those indexes, it is a database
/// generated eight-byte identity rather than a sixteen-byte UUID: half the
/// bytes per index entry, and keys minted close together sort close together
/// on the physical page. The key is assigned once by the database and never
/// reassigned — a reused number would attach one stream's history to another
/// — and it is never rendered to a client.
#[derive(Clone, Copy, Debug, Display, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[display("{_0}")]
pub struct StreamKey(i64);

impl StreamKey {
    /// Return the stored representation.
    pub const fn get(self) -> i64 {
        self.0
    }
}

impl TryFrom<i64> for StreamKey {
    type Error = StreamKeyOutOfRange;

    fn try_from(stored: i64) -> Result<StreamKey, StreamKeyOutOfRange> {
        if stored > 0 { Ok(StreamKey(stored)) } else { Err(StreamKeyOutOfRange) }
    }
}

/// A stream key was outside the range the database assigns.
#[derive(Clone, Copy, Debug, Eq, Error, Hash, PartialEq)]
#[error("a stream key is a positive number")]
pub struct StreamKeyOutOfRange;

#[cfg(test)]
mod tests {
    use crate::stream_key::{StreamKey, StreamKeyOutOfRange};

    #[test]
    fn a_stored_key_round_trips() {
        assert_eq!(StreamKey::try_from(4_294_967_296_i64).map(StreamKey::get), Ok(4_294_967_296));
    }

    #[test]
    fn zero_and_negative_keys_are_refused() {
        assert_eq!(StreamKey::try_from(0), Err(StreamKeyOutOfRange));
        assert_eq!(StreamKey::try_from(-1), Err(StreamKeyOutOfRange));
    }

    #[test]
    fn a_key_renders_as_the_number_it_stores() {
        assert_eq!(StreamKey::try_from(17).map(|key| key.to_string()), Ok("17".to_owned()));
    }
}
