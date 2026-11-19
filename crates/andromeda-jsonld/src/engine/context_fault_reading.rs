//! Reading the JSON-LD processor's refusals of a `@context` as
//! [`ContextFault`]s.
//!
//! The processor locates a syntax refusal in the keys leading to the member it
//! refused, and a processing refusal in the term whose definition failed. This
//! is where the processor's own JSON kinds are named, so the reading of both
//! lives here.

use crate::{
    context_fault::{ContextFault, ContextLocation},
    context_subtree::UnusableLocalContext,
    loader::error::InvalidContextDocument,
};
use andromeda_json_literal::piece::JsonKind;
use iri_rs::InvalidIriRef;
use jsonld::{ExtractContextError, context_processing::Error as ProcessingError};
use jsonld_syntax::context::{
    InvalidContext,
    definition::{KeyOrKeyword, KeyOrKeywordRef},
};
use std::sync::Arc;

/// The fault a refusal of the `@context` syntax reports.
pub(crate) fn syntax_fault(error: &InvalidContext) -> ContextFault {
    let mut keys = Vec::new();
    let mut cause = error;
    while let InvalidContext::Entry { key, error } = cause {
        keys.push(key.clone()); // clone: the fault outlives the refusal it is read from
        cause = error;
    }
    let location = ContextLocation::new(keys.into_boxed_slice());
    let Some(last) = location.keys().last() else {
        return unlocated_fault(cause);
    };
    let last_is_term = matches!(last, KeyOrKeyword::Key(_));
    match cause {
        InvalidContext::Unexpected(found, _) if last_is_term => ContextFault::TermDefinitionKind {
            term: location,
            found: json_kind(*found),
        },
        InvalidContext::Unexpected(found, expected) => ContextFault::MemberKind {
            member: location,
            found: json_kind(*found),
            expected: expected.iter().copied().map(json_kind).collect(),
        },
        InvalidContext::InvalidIriRef(_) => ContextFault::MemberNotUrl { member: location },
        InvalidContext::DuplicateKey => ContextFault::DuplicateMember { member: location },
        InvalidContext::InvalidTermDefinition if last_is_term => ContextFault::DisallowedMember { member: location },
        InvalidContext::InvalidTermDefinition => ContextFault::DisallowedValue { member: location },
        InvalidContext::InvalidDirection => ContextFault::Direction { member: location },
        InvalidContext::InvalidNestValue(_) => ContextFault::NestValue { member: location },
        InvalidContext::TooDeep => ContextFault::TooDeep,
        InvalidContext::Entry { .. } => ContextFault::Invalid { reason: cause.code() },
    }
}

/// The fault a syntax refusal reports when no entry locates it: one of the
/// entries of the `@context` itself is refused.
fn unlocated_fault(cause: &InvalidContext) -> ContextFault {
    match cause {
        InvalidContext::InvalidIriRef(reference) => ContextFault::ReferenceNotUrl {
            reference: Arc::new(InvalidIriRef(reference.clone())), // clone: the fault outlives the refusal it is read from
        },
        InvalidContext::TooDeep => ContextFault::TooDeep,
        InvalidContext::Entry { .. }
        | InvalidContext::Unexpected(..)
        | InvalidContext::InvalidDirection
        | InvalidContext::DuplicateKey
        | InvalidContext::InvalidTermDefinition
        | InvalidContext::InvalidNestValue(_) => ContextFault::Invalid { reason: cause.code() },
    }
}

/// The fault a refusal of the JSON-LD 1.1 context processing algorithm
/// reports, located in the term whose definition failed when there is one.
///
/// The processor names every binding of a context definition as a term, a
/// binding spelled as a keyword included, so the located key is read again by
/// its spelling, as a syntax refusal's keys are.
///
/// Written as `if let` rather than an exhaustive match because
/// `ProcessingError` is a foreign enum of twenty-odd refusals, every one of
/// which other than the located one is named by its JSON-LD error code alone.
pub(crate) fn processing_fault<E>(error: &ProcessingError<E>) -> ContextFault {
    if let ProcessingError::TermDefinition { term, error } = error {
        ContextFault::TermDefinition {
            term: KeyOrKeywordRef::from(term.as_str()).to_owned(),
            reason: error.code(),
        }
    } else {
        ContextFault::Invalid { reason: error.code() }
    }
}

/// Why no context could be read from a document the processor loaded.
pub(crate) fn extraction_failure(error: &ExtractContextError) -> InvalidContextDocument {
    match error {
        ExtractContextError::Unexpected(_) => InvalidContextDocument::NoContext(UnusableLocalContext::NotAnObject),
        ExtractContextError::NoContext => InvalidContextDocument::NoContext(UnusableLocalContext::NoContextMember),
        ExtractContextError::DuplicateContext => InvalidContextDocument::DuplicateContext,
        ExtractContextError::Syntax(invalid) => InvalidContextDocument::InvalidContext(syntax_fault(invalid)),
    }
}

/// The kind of JSON value the processor's reader names.
const fn json_kind(kind: jstrict::Kind) -> JsonKind {
    match kind {
        jstrict::Kind::Null => JsonKind::Null,
        jstrict::Kind::Boolean => JsonKind::Boolean,
        jstrict::Kind::Number => JsonKind::Number,
        jstrict::Kind::String => JsonKind::String,
        jstrict::Kind::Array => JsonKind::Array,
        jstrict::Kind::Object => JsonKind::Object,
    }
}

#[cfg(test)]
#[expect(clippy::absolute_paths, reason = "clippy reads a nested item of a grouped import as an inline path")]
mod tests {
    use crate::{
        context_fault::{ContextFault, ContextLocation},
        engine::context_fault_reading::{processing_fault, syntax_fault},
    };
    use andromeda_json_literal::piece::JsonKind;
    use jsonld::{
        NoLoader,
        context_processing::{Error as ProcessingError, Process},
        rdfx::vocabulary::no_vocabulary_mut,
        syntax::{
            ErrorCode,
            Parse,
            TryFromJson,
            Value,
            context::{Context, definition::KeyOrKeywordRef},
        },
    };
    use std::convert::Infallible;
    use tokio::runtime::Builder;

    /// The fault of the `@context` written as `json`: its syntax refusal, or
    /// else its processing refusal.
    fn fault(json: &str) -> ContextFault {
        let (value, _) = Value::parse_str(json).expect("JSON");
        let context = match Context::try_from_json(&value) {
            Ok(context) => context,
            Err(invalid) => return syntax_fault(&invalid),
        };
        let runtime = Builder::new_current_thread().build().expect("runtime");
        match runtime.block_on(context.process(no_vocabulary_mut(), &NoLoader, None)) {
            Ok(_) => panic!("the context {json} processes"),
            Err(error) => processing_fault(&error),
        }
    }

    fn rendered(json: &str) -> String {
        format!("the @context {}", fault(json))
    }

    /// The seven bodies the Add @context refusal is specified by, and a
    /// redefined keyword (JSON-LD 1.1 create term definition step 5), each
    /// named by the term or keyword it is wrong in.
    #[test]
    fn every_invalid_context_names_what_is_wrong_in_it() {
        for (context, detail) in [
            (
                r#"{"speed": 5}"#,
                "the @context defines the term speed as a number, but a term definition is null, a string or an object",
            ),
            (
                r#"{"speed": true}"#,
                "the @context defines the term speed as a boolean, but a term definition is null, a string or an object",
            ),
            (r#""not a url""#, r#"the @context names "not a url", which is not a URL"#),
            (
                r#"{"@type": "https://example.org/t"}"#,
                "the @context has a member @type that is a string, but an object is expected",
            ),
            (
                r#"{"speed": {"@id": 5}}"#,
                "the @context has a member speed/@id that is a number, but a string is expected",
            ),
            (r#"{"a": "b:x", "b": "a:y"}"#, "the @context cannot define the term a: cyclic IRI mapping"),
            (
                r#"{"@id": "https://example.org/id"}"#,
                "the @context cannot define the keyword @id: keyword redefinition",
            ),
            (
                r#"{"speed": {"@id": "https://example.org/speed", "@container": "@bogus"}}"#,
                "the @context has a member speed/@container holding a value JSON-LD does not allow there",
            ),
        ] {
            pretty_assertions::assert_eq!(rendered(context), detail, "{context}");
        }
    }

    #[test]
    fn an_entry_a_term_definition_does_not_take_is_disallowed() {
        pretty_assertions::assert_eq!(
            rendered(r#"{"speed": {"@id": "https://example.org/speed", "unit": "km/h"}}"#),
            "the @context has a member speed/unit that JSON-LD does not allow there"
        );
    }

    #[test]
    fn a_fault_inside_a_scoped_context_is_located_through_the_scoping_term() {
        let scoped = ContextLocation::new(
            ["speed", "@context", "gear"]
                .into_iter()
                .map(|key| KeyOrKeywordRef::from(key).to_owned())
                .collect(),
        );
        let fault = fault(r#"{"speed": {"@id": "https://example.org/speed", "@context": {"gear": 3}}}"#);
        assert!(
            matches!(&fault, ContextFault::TermDefinitionKind { term, found: JsonKind::Number } if *term == scoped),
            "{fault}"
        );
    }

    #[test]
    fn a_processing_refusal_with_no_term_is_named_by_its_code() {
        assert!(matches!(
            processing_fault::<Infallible>(&ProcessingError::InvalidBaseIri),
            ContextFault::Invalid {
                reason: ErrorCode::InvalidBaseIri
            }
        ));
    }
}
