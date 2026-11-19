//! What a decoding derived from a type's declaration refuses on its own.
//!
//! A decoding the declaration of a type derives refuses four things itself: a
//! value that is none of the tokens a member takes, a member the object does
//! not take, a member it requires that is absent, and a member written twice.
//! Each is held typed, with the value written and the names the declaration
//! gives, and worded in the members and values of the request rather than in
//! the decoder's own words.

use andromeda_ngsi_ld_query::defined_token::{Vocabulary, WrittenWord};
use derive_more::Display;
use thiserror::Error;

/// A member name a type's declaration gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
#[display("'{_0}'")]
pub struct DeclaredName(pub &'static str);

/// A refusal a derived decoding raises on its own.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DecodeFault {
    /// A value that is none of the tokens the member takes.
    #[error("{given} is not one of {takes}")]
    UnknownValue {
        /// The value as written.
        given: WrittenWord,
        /// The tokens the member takes.
        takes: Vocabulary,
    },
    /// A member the object does not take.
    #[error("{given} is not a member it takes, which are {takes}")]
    UnknownMember {
        /// The member name as written.
        given: WrittenWord,
        /// The members the object takes.
        takes: Vocabulary,
    },
    /// A member the object requires is absent.
    #[error("the member {0} is missing")]
    MissingMember(DeclaredName),
    /// A member is written more than once.
    #[error("the member {0} is given more than once")]
    RepeatedMember(DeclaredName),
}

#[cfg(test)]
mod tests {
    use crate::decode_fault::{DeclaredName, DecodeFault};
    use andromeda_ngsi_ld_query::defined_token::{Vocabulary, WrittenWord};

    #[test]
    fn each_fault_names_what_was_written_and_what_is_taken() {
        for (fault, detail) in [
            (
                DecodeFault::UnknownValue {
                    given: WrittenWord::from("bogus".to_owned()),
                    takes: Vocabulary(&["inclusive", "exclusive"]),
                },
                "'bogus' is not one of inclusive, exclusive",
            ),
            (
                DecodeFault::UnknownMember {
                    given: WrittenWord::from("protcol".to_owned()),
                    takes: Vocabulary(&["uri", "accept"]),
                },
                "'protcol' is not a member it takes, which are uri, accept",
            ),
            (DecodeFault::MissingMember(DeclaredName("type")), "the member 'type' is missing"),
            (DecodeFault::RepeatedMember(DeclaredName("uri")), "the member 'uri' is given more than once"),
        ] {
            pretty_assertions::assert_eq!(fault.to_string(), detail);
        }
    }

    /// A value is written back on one line, whatever it holds.
    #[test]
    fn a_written_value_is_escaped_onto_one_line() {
        let fault = DecodeFault::UnknownValue {
            given: WrittenWord::from("a\nb'c".to_owned()),
            takes: Vocabulary(&[]),
        };
        pretty_assertions::assert_eq!(fault.to_string(), r"'a\nb\'c' is not one of none");
    }
}
