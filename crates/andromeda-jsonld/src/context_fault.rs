//! Where and why a `@context` is not valid JSON-LD.
//!
//! The JSON-LD processor refuses a `@context` while reading its syntax (a
//! member holding a value of the wrong kind, a reference that is not an IRI)
//! and while processing it (the JSON-LD 1.1 create term definition
//! algorithm). Either way it names the member, term or keyword it refused.
//! [`ContextFault`] keeps that location and the reason, typed, so a client is
//! told in prose which part of its `@context` is wrong rather than the
//! processor's own message chain.
//!
//! A fault reads as a predicate of the `@context` it was found in, so the
//! context can be named in front of it ([`FaultyContext`]).
//!
//! A term is named as a JSON string, quoted and escaped, because any key
//! may be one: the empty key reads `""` and a term spelled like a keyword
//! reads apart from the keyword. A keyword is named bare.

use crate::context_spec::ContextSpec;
use andromeda_json_literal::{json_text::quoted, piece::JsonKind};
use derive_more::Display;
use iri_rs::{InvalidIriRef, IriBuf};
use jsonld_syntax::{ErrorCode, context::definition::KeyOrKeyword};
use std::{fmt, sync::Arc};
use thiserror::Error;

/// What is wrong with a `@context`, and where.
#[derive(Debug, Clone, Error)]
pub enum ContextFault {
    /// A term is defined by a value of a kind no term definition takes.
    ///
    /// JSON-LD 1.1 section 9.15: a term definition is `null`, a string, or an
    /// expanded term definition, which is an object.
    #[error("defines the term {term} as {}, but a term definition is null, a string or an object", WithArticle(.found))]
    TermDefinitionKind {
        /// The term, below the keys of the scoped contexts it is defined in.
        term: ContextLocation,
        /// The kind of value the term is defined by.
        found: JsonKind,
    },
    /// A member holds a value of a kind it does not take.
    #[error("has a member {member} that is {}, but {} is expected", WithArticle(.found), ExpectedKinds(.expected))]
    MemberKind {
        /// The member.
        member: ContextLocation,
        /// The kind of value the member holds.
        found: JsonKind,
        /// The kinds the member takes.
        expected: Box<[JsonKind]>,
    },
    /// An entry of the `@context` names a remote context by a reference that
    /// is not an IRI reference.
    #[error("names \"{}\", which is not a URL", .reference.0)]
    ReferenceNotUrl {
        /// The reference as written.
        ///
        /// Shared because the refusal it is read from is not cloneable and
        /// the fault travels in errors that are.
        reference: Arc<InvalidIriRef<String>>,
    },
    /// A member that takes an IRI reference holds text that is not one.
    #[error("has a member {member} that is not a URL")]
    MemberNotUrl {
        /// The member.
        member: ContextLocation,
    },
    /// A member is written more than once in one object.
    #[error("has the member {member} more than once")]
    DuplicateMember {
        /// The member.
        member: ContextLocation,
    },
    /// A member the JSON-LD 1.1 grammar does not allow where it is written,
    /// such as an entry of a term definition that is no keyword.
    #[error("has a member {member} that JSON-LD does not allow there")]
    DisallowedMember {
        /// The member.
        member: ContextLocation,
    },
    /// A keyword member holding a value the JSON-LD 1.1 grammar does not allow
    /// for that keyword, such as a `@container` naming no container.
    #[error("has a member {member} holding a value JSON-LD does not allow there")]
    DisallowedValue {
        /// The member.
        member: ContextLocation,
    },
    /// A `@direction` member is neither `"ltr"` nor `"rtl"`.
    #[error("has a member {member} that is neither \"ltr\" nor \"rtl\"")]
    Direction {
        /// The member.
        member: ContextLocation,
    },
    /// A `@nest` member names a keyword other than `@nest`.
    #[error("has a member {member} naming a keyword other than @nest")]
    NestValue {
        /// The member.
        member: ContextLocation,
    },
    /// Scoped contexts nest deeper than the processor reads.
    #[error("nests scoped contexts deeper than the JSON-LD processor reads")]
    TooDeep,
    /// The JSON-LD 1.1 create term definition algorithm refuses the definition
    /// of a term, or of a keyword the `@context` tries to define.
    #[error("cannot define {}: {reason}", Defined(.term))]
    TermDefinition {
        /// The term or keyword.
        term: KeyOrKeyword,
        /// The JSON-LD error the definition raised.
        reason: ErrorCode,
    },
    /// The `@context` is refused for a reason no one member carries.
    #[error("is not a valid JSON-LD context: {reason}")]
    Invalid {
        /// The JSON-LD error the `@context` raised.
        reason: ErrorCode,
    },
}

/// The `@context` a [`ContextFault`] was found in.
#[derive(Debug, Clone, PartialEq, Eq, Display)]
pub enum FaultyContext {
    /// A context a request named by URL.
    #[display("the @context {_0}")]
    Url(IriBuf),
    /// A context with no URL to name it by: one written in place, or one the
    /// failing operation did not say.
    #[display("the @context")]
    Unnamed,
}

impl FaultyContext {
    /// The context a request's `@context` is named by: the URL its `Link`
    /// header names, or none for a context the request body writes.
    #[must_use]
    pub fn named_by(spec: &ContextSpec) -> FaultyContext {
        spec.link_context().map_or(FaultyContext::Unnamed, |url| {
            FaultyContext::Url(url.clone()) // clone: the fault outlives the request's spec
        })
    }
}

/// The keys leading to a member of a `@context`, outermost first: a term or
/// keyword of the `@context` itself, then members of that term's definition
/// and of the scoped contexts below it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextLocation(Box<[KeyOrKeyword]>);

impl ContextLocation {
    /// The location the keys `keys` lead to, outermost first.
    #[must_use]
    pub const fn new(keys: Box<[KeyOrKeyword]>) -> ContextLocation {
        ContextLocation(keys)
    }

    /// The keys, outermost first.
    #[must_use]
    pub fn keys(&self) -> &[KeyOrKeyword] {
        &self.0
    }
}

// Written by hand: the keys are joined, each term quoted and each keyword
// bare, which no derive spells.
impl fmt::Display for ContextLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (position, key) in self.0.iter().enumerate() {
            if position > 0 {
                f.write_str("/")?;
            }
            match key {
                KeyOrKeyword::Keyword(keyword) => write!(f, "{keyword}")?,
                KeyOrKeyword::Key(term) => f.write_str(&quoted(term.as_str()))?,
            }
        }
        Ok(())
    }
}

/// A term or keyword a `@context` defines, named as prose names it.
struct Defined<'a>(&'a KeyOrKeyword);

// Written by hand: a keyword and a term are introduced by different words.
impl fmt::Display for Defined<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            KeyOrKeyword::Keyword(keyword) => write!(f, "the keyword {keyword}"),
            KeyOrKeyword::Key(term) => write!(f, "the term {}", quoted(term.as_str())),
        }
    }
}

/// The kinds of JSON value a member takes, as a list in prose.
struct ExpectedKinds<'a>(&'a [JsonKind]);

// Written by hand: the list is joined with commas and a final "or".
impl fmt::Display for ExpectedKinds<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let last = self.0.len().saturating_sub(1);
        for (position, kind) in self.0.iter().enumerate() {
            if position == last && position > 0 {
                f.write_str(" or ")?;
            } else if position > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{}", WithArticle(kind))?;
        }
        Ok(())
    }
}

/// A kind of JSON value as prose names one value of it.
struct WithArticle<'a>(&'a JsonKind);

// Written by hand: each kind takes its own article, which no derive spells.
impl fmt::Display for WithArticle<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self.0 {
            JsonKind::Null => "null",
            JsonKind::Boolean => "a boolean",
            JsonKind::Number => "a number",
            JsonKind::String => "a string",
            JsonKind::Array => "an array",
            JsonKind::Object => "an object",
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::context_fault::{ContextFault, ContextLocation, FaultyContext};
    use andromeda_json_literal::piece::JsonKind;
    use iri_rs::{InvalidIriRef, IriBuf};
    use jsonld_syntax::{
        ErrorCode,
        Keyword,
        context::definition::{Key, KeyOrKeyword},
    };
    use std::sync::Arc;

    fn location(keys: &[&str]) -> ContextLocation {
        ContextLocation::new(
            keys.iter()
                .map(|key| match Keyword::try_from(*key) {
                    Ok(keyword) => KeyOrKeyword::Keyword(keyword),
                    Err(_) => KeyOrKeyword::Key(Key::from(*key)),
                })
                .collect(),
        )
    }

    /// JSON-LD 1.1 section 9.15 says what a term definition may be, and the
    /// fault says it after naming the term.
    #[test]
    fn a_term_defined_by_a_number_names_the_term_and_what_a_term_definition_may_be() {
        let fault = ContextFault::TermDefinitionKind {
            term: location(&["speed"]),
            found: JsonKind::Number,
        };
        pretty_assertions::assert_eq!(
            format!("{} {fault}", FaultyContext::Unnamed),
            r#"the @context defines the term "speed" as a number, but a term definition is null, a string or an object"#
        );
    }

    #[test]
    fn a_member_of_the_wrong_kind_names_the_member_and_the_kinds_it_takes() {
        let fault = ContextFault::MemberKind {
            member: location(&["speed", "@id"]),
            found: JsonKind::Number,
            expected: Box::new([JsonKind::String]),
        };
        pretty_assertions::assert_eq!(fault.to_string(), r#"has a member "speed"/@id that is a number, but a string is expected"#);

        let fault = ContextFault::MemberKind {
            member: location(&["@base"]),
            found: JsonKind::Boolean,
            expected: Box::new([JsonKind::Null, JsonKind::String, JsonKind::Object]),
        };
        pretty_assertions::assert_eq!(
            fault.to_string(),
            "has a member @base that is a boolean, but null, a string or an object is expected"
        );
    }

    /// A term is named in quotes and a keyword bare.
    #[test]
    fn a_refused_term_definition_names_the_term_and_the_json_ld_error() {
        let fault = ContextFault::TermDefinition {
            term: KeyOrKeyword::Key(Key::from("beginLifespanVersion")),
            reason: ErrorCode::InvalidIriMapping,
        };
        pretty_assertions::assert_eq!(fault.to_string(), r#"cannot define the term "beginLifespanVersion": invalid IRI mapping"#);

        let fault = ContextFault::TermDefinition {
            term: KeyOrKeyword::Keyword(Keyword::Id),
            reason: ErrorCode::KeywordRedefinition,
        };
        pretty_assertions::assert_eq!(fault.to_string(), "cannot define the keyword @id: keyword redefinition");
    }

    /// JSON-LD 1.1 API section 4.2.2, create term definition step 2: the
    /// empty string is no term, and the refusal names it as `""`.
    #[test]
    fn a_refused_definition_of_the_empty_term_names_it_as_an_empty_string() {
        let fault = ContextFault::TermDefinition {
            term: KeyOrKeyword::Key(Key::from("")),
            reason: ErrorCode::InvalidTermDefinition,
        };
        pretty_assertions::assert_eq!(fault.to_string(), r#"cannot define the term "": invalid term definition"#);
    }

    /// A term written like a keyword is not one, so it reads quoted beside
    /// the keyword it resembles, and a term holding a quotation mark reads
    /// escaped.
    #[test]
    fn a_member_path_quotes_each_term_and_leaves_each_keyword_bare() {
        let fault = ContextFault::DuplicateMember {
            member: location(&["", "@context", "@vocabulary", "a\"b", "@id"]),
        };
        pretty_assertions::assert_eq!(fault.to_string(), r#"has the member ""/@context/"@vocabulary"/"a\"b"/@id more than once"#);
    }

    #[test]
    fn a_context_named_by_url_is_named_in_front_of_the_fault() {
        let url = IriBuf::new("https://example.org/context.jsonld".to_owned()).expect("a valid IRI");
        let fault = ContextFault::ReferenceNotUrl {
            reference: Arc::new(InvalidIriRef("not a url".to_owned())),
        };
        pretty_assertions::assert_eq!(
            format!("{} {fault}", FaultyContext::Url(url)),
            r#"the @context https://example.org/context.jsonld names "not a url", which is not a URL"#
        );
    }
}
