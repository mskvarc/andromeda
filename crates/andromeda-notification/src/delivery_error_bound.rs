//! The length a recorded delivery failure is trimmed to before it is stored.
//!
//! The schema, not the worker, owns the ceiling: the migration that creates the
//! delivery queue constrains `delivery_error`, so a diagnostic longer than that
//! would fail the write and lose the retry bookkeeping travelling with it. The
//! bound is applied at write time — on the way into the store, never on the way
//! out — so what is read back is exactly what was persisted.

use std::{borrow::Cow, num::NonZeroU32};
use thiserror::Error;

/// Rejected delivery-error truncation bound.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum DeliveryErrorBoundError {
    /// A zero-character bound discards every diagnostic, leaving a failed
    /// delivery with no recorded reason at all.
    #[error("notification delivery error bound must be at least one character")]
    ZeroCharacters,
    /// A bound above what the schema constrains would let a write fail on a
    /// diagnostic instead of on the delivery outcome it accompanies.
    #[error("notification delivery error bound of {characters} characters exceeds the schema maximum of {maximum} characters")]
    AboveSchemaMaximum {
        /// The rejected bound.
        characters: u32,
        /// The largest bound the schema accepts.
        maximum: u32,
    },
}

/// The maximum number of characters of a delivery failure that is stored.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct DeliveryErrorBound(NonZeroU32);

impl DeliveryErrorBound {
    /// The largest bound the delivery schema accepts.
    pub const SCHEMA_MAXIMUM_CHARACTERS: u32 = 2_048;

    /// Build a bound from a whole number of characters.
    ///
    /// # Errors
    ///
    /// Returns [`DeliveryErrorBoundError::ZeroCharacters`] when `characters` is
    /// zero, and [`DeliveryErrorBoundError::AboveSchemaMaximum`] when it exceeds
    /// [`DeliveryErrorBound::SCHEMA_MAXIMUM_CHARACTERS`].
    pub const fn from_characters(characters: u32) -> Result<DeliveryErrorBound, DeliveryErrorBoundError> {
        if characters > DeliveryErrorBound::SCHEMA_MAXIMUM_CHARACTERS {
            return Err(DeliveryErrorBoundError::AboveSchemaMaximum {
                characters,
                maximum: DeliveryErrorBound::SCHEMA_MAXIMUM_CHARACTERS,
            });
        }
        match NonZeroU32::new(characters) {
            Some(characters) => Ok(DeliveryErrorBound(characters)),
            None => Err(DeliveryErrorBoundError::ZeroCharacters),
        }
    }

    /// The bound as a whole number of characters.
    #[must_use]
    pub const fn characters(self) -> u32 {
        self.0.get()
    }

    /// Trim `detail` to the bound, borrowing it unchanged when it already fits.
    ///
    /// Truncation counts characters and cuts on a character boundary, so a
    /// multi-byte diagnostic is shortened rather than split into invalid UTF-8.
    #[must_use]
    pub fn truncate(self, detail: &str) -> Cow<'_, str> {
        let Ok(limit) = usize::try_from(self.characters()) else {
            return Cow::Borrowed(detail);
        };
        match detail.char_indices().nth(limit) {
            Some((cut, _)) => Cow::Owned(detail[..cut].to_owned()),
            None => Cow::Borrowed(detail),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::delivery_error_bound::{DeliveryErrorBound, DeliveryErrorBoundError};
    use pretty_assertions::assert_eq;
    use std::borrow::Cow;

    #[test]
    fn a_zero_character_bound_is_rejected() {
        assert_eq!(DeliveryErrorBound::from_characters(0), Err(DeliveryErrorBoundError::ZeroCharacters));
    }

    #[test]
    fn a_bound_above_the_schema_maximum_is_rejected() {
        assert_eq!(
            DeliveryErrorBound::from_characters(2_049),
            Err(DeliveryErrorBoundError::AboveSchemaMaximum {
                characters: 2_049,
                maximum: 2_048,
            })
        );
    }

    #[test]
    fn a_diagnostic_within_the_bound_is_borrowed_unchanged() -> Result<(), DeliveryErrorBoundError> {
        let bound = DeliveryErrorBound::from_characters(16)?;
        assert_eq!(bound.truncate("endpoint refused"), Cow::Borrowed("endpoint refused"));
        Ok(())
    }

    #[test]
    fn a_diagnostic_longer_than_the_bound_is_cut_to_it() -> Result<(), DeliveryErrorBoundError> {
        let bound = DeliveryErrorBound::from_characters(8)?;
        assert_eq!(bound.truncate("endpoint refused"), Cow::<str>::Owned("endpoint".to_owned()));
        Ok(())
    }

    #[test]
    fn truncation_cuts_on_a_character_boundary_rather_than_a_byte_boundary() -> Result<(), DeliveryErrorBoundError> {
        let bound = DeliveryErrorBound::from_characters(3)?;
        let truncated = bound.truncate("\u{e9}\u{e9}\u{e9}\u{e9}");
        assert_eq!(truncated, Cow::<str>::Owned("\u{e9}\u{e9}\u{e9}".to_owned()));
        assert_eq!(truncated.chars().count(), 3);
        Ok(())
    }

    #[test]
    fn a_bound_reports_the_character_count_it_was_built_from() -> Result<(), DeliveryErrorBoundError> {
        assert_eq!(DeliveryErrorBound::from_characters(1_024)?.characters(), 1_024);
        Ok(())
    }
}
