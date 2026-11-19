//! The error a watched visitor raises its refusals through.

use crate::decode_fault::{DeclaredName, DecodeFault};
use andromeda_ngsi_ld_query::defined_token::{Vocabulary, WrittenWord};
use serde::de::{Error as DecodeError, Expected, Unexpected};
use std::fmt::Display;
use thiserror::Error;

/// The reader's own error for a refusal, and the fault it stands for when the
/// declaration raised one of its own.
///
/// Every refusal is first built as the reader builds it, so the reader hands
/// back the same failure, in the same words, as it would unwatched.
#[derive(Debug, Error)]
#[error("{inner}")]
pub(crate) struct Noted<E> {
    /// The reader's own error.
    pub(super) inner: E,
    /// The fault the declaration raised, when it raised one.
    pub(super) fault: Option<DecodeFault>,
}

impl<E> Noted<E> {
    /// A failure passed on from the reader, whose fault, if any, was noted
    /// where it was raised.
    pub(super) const fn passed(inner: E) -> Noted<E> {
        Noted { inner, fault: None }
    }

    /// The reader's error `inner`, raised for `fault`.
    const fn raised(inner: E, fault: DecodeFault) -> Noted<E> {
        Noted { inner, fault: Some(fault) }
    }
}

impl<E: DecodeError> DecodeError for Noted<E> {
    fn custom<T: Display>(message: T) -> Noted<E> {
        Noted::passed(E::custom(message))
    }

    fn invalid_type(unexpected: Unexpected<'_>, expected: &dyn Expected) -> Noted<E> {
        Noted::passed(E::invalid_type(unexpected, expected))
    }

    fn invalid_value(unexpected: Unexpected<'_>, expected: &dyn Expected) -> Noted<E> {
        Noted::passed(E::invalid_value(unexpected, expected))
    }

    fn invalid_length(length: usize, expected: &dyn Expected) -> Noted<E> {
        Noted::passed(E::invalid_length(length, expected))
    }

    fn unknown_variant(variant: &str, expected: &'static [&'static str]) -> Noted<E> {
        Noted::raised(
            E::unknown_variant(variant, expected),
            DecodeFault::UnknownValue {
                given: WrittenWord::from(variant.to_owned()),
                takes: Vocabulary(expected),
            },
        )
    }

    fn unknown_field(field: &str, expected: &'static [&'static str]) -> Noted<E> {
        Noted::raised(
            E::unknown_field(field, expected),
            DecodeFault::UnknownMember {
                given: WrittenWord::from(field.to_owned()),
                takes: Vocabulary(expected),
            },
        )
    }

    fn missing_field(field: &'static str) -> Noted<E> {
        Noted::raised(E::missing_field(field), DecodeFault::MissingMember(DeclaredName(field)))
    }

    fn duplicate_field(field: &'static str) -> Noted<E> {
        Noted::raised(E::duplicate_field(field), DecodeFault::RepeatedMember(DeclaredName(field)))
    }
}
