//! Carrying a typed refusal out of a serde decode.
//!
//! A serde visitor can only fail with its deserializer's own error type, which
//! erases what went wrong into a message. The decoder therefore records the
//! typed [`EntityRefusal`] here when it refuses, fails the visitor with that
//! refusal's message, and the entry points hand the recorded refusal back
//! instead of the erased error.

use crate::refusal::EntityRefusal;
use serde::de::Error;
use std::cell::RefCell;

/// The refusal of one decode, once one was raised.
#[derive(Debug, Default)]
pub(crate) struct DecodeContext {
    /// The first refusal raised; later ones are consequences of it.
    refusal: RefCell<Option<EntityRefusal>>,
}

impl DecodeContext {
    /// Record `refusal` and turn it into the deserializer's error.
    pub(crate) fn refuse<E: Error>(&self, refusal: EntityRefusal) -> E {
        let error = E::custom(&refusal);
        let mut recorded = self.refusal.borrow_mut();
        if recorded.is_none() {
            *recorded = Some(refusal);
        }
        error
    }

    /// The recorded refusal, or the reader's own failure when the decoder
    /// refused nothing itself.
    pub(crate) fn into_refusal(self, error: &serde_json::Error) -> EntityRefusal {
        self.refusal.into_inner().unwrap_or_else(|| EntityRefusal::syntax(error))
    }
}
