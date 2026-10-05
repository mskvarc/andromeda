//! A request body read from its text, held until the user `@context` it was
//! submitted with is admitted.
//!
//! Clause 5.5.4 judges a payload "syntactically correct, according to the
//! @context", so the context is resolved before anything is judged under it:
//! a context that cannot be retrieved is clause 5.5.6's
//! `LdContextNotAvailable`, and clause 5.5.7 refuses a scoped or an embedded
//! user `@context` before the payload's own rules are applied. The body is
//! still decoded where the extractor reads it, on the blocking pool when it is
//! large; only the verdict waits. A [`SubmittedBody`] yields its value for an
//! [`AdmittedContext`], or, for an operation that never resolves its context,
//! once that context was found to embed nothing.

use andromeda_http_response::{classified_report::ClassifiedReport, problem_response::ProblemResponse};
use andromeda_jsonld::user_context_admission::{AdmittedContext, SubmittedContext};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::attribute::{member_reading::MemberReading, written_attribute::WrittenAttribute};
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use axum::http::Uri;

/// A request body decoded into `T`, or refused, not yet handed out.
#[must_use = "a submitted body is read once its context is admitted"]
pub struct SubmittedBody<T> {
    /// The decoded body, or the refusal its payload rules gave it.
    decoded: Result<T, ProblemResponse>,
    /// The request URI, the `instance` of a refusal of the embedded context.
    uri: Uri,
}

impl<T> SubmittedBody<T> {
    /// The body `decoded` into, or the refusal of it, for the request `uri`.
    pub(crate) const fn new(decoded: Result<T, ProblemResponse>, uri: Uri) -> SubmittedBody<T> {
        SubmittedBody { decoded, uri }
    }

    /// The body read under `context`, which the operation admitted first.
    ///
    /// # Errors
    ///
    /// Returns the refusal the body's payload rules gave it.
    pub fn admitted(self, _context: &AdmittedContext) -> Result<T, ProblemResponse> {
        self.decoded
    }

    /// The body of an operation that never resolves `context`, refused first
    /// when the body embedded a `@context` below its top level (clause 5.5.7).
    ///
    /// # Errors
    ///
    /// Returns a `BadRequestData` locating the embedded `@context`, then the
    /// refusal the body's payload rules gave it.
    pub fn context_free(self, context: SubmittedContext) -> Result<T, ProblemResponse> {
        let SubmittedBody { decoded, uri } = self;
        context
            .unused()
            .map_err(|embedded| ClassifiedReport::public(NgsiLdErrorType::BadRequestData, embedded.to_string()).with_instance(uri))?;
        decoded
    }

    /// The body `map` makes of the decoded one, refused as `map` refuses it.
    pub(crate) fn and_then<U>(self, map: impl FnOnce(T, &Uri) -> Result<U, ProblemResponse>) -> SubmittedBody<U> {
        let SubmittedBody { decoded, uri } = self;
        let decoded = decoded.and_then(|value| map(value, &uri));
        SubmittedBody { decoded, uri }
    }
}

impl SubmittedBody<WrittenAttribute> {
    /// The member the bare Attribute Fragment of clauses 5.6.4 and 5.6.19 is,
    /// read under the attribute name `attribute` the request path carries.
    ///
    /// A fragment no clause admits where an attribute stands is refused as
    /// `BadRequestData` (clause 5.5.4), once the request's context was admitted
    /// like any other refusal of the body.
    pub fn read_attribute<M: MemberReading>(self, attribute: &AttributeName) -> SubmittedBody<M> {
        self.and_then(|written, uri| {
            written.read(attribute).map_err(|refused| {
                // clone: the refusal names the request, which the held body keeps naming
                ClassifiedReport::public(NgsiLdErrorType::BadRequestData, refused.to_string()).with_instance(uri.clone())
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::submitted_body::SubmittedBody;
    use andromeda_http_response::{classified_report::ClassifiedReport, problem_response::ProblemResponse};
    use andromeda_jsonld::{
        context_spec::ContextSpec,
        user_context_admission::{AdmittedContext, SubmittedContext},
    };
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
    use andromeda_ngsi_ld_representation::embedded_context_admission::find_embedded_context;
    use axum::http::Uri;
    use std::fmt::Debug;

    fn uri() -> Uri {
        Uri::from_static("/ngsi-ld/v1/entityMaps/urn:ngsi-ld:EntityMap:1")
    }

    /// A context submitted with a `@context` embedded under `/speed`.
    fn embedding() -> SubmittedContext {
        let found = find_embedded_context(br#"{"speed": {"@context": {}}}"#).expect("valid JSON");
        SubmittedContext::new(ContextSpec::none(), found)
    }

    /// A context submitted embedding nothing.
    fn plain() -> SubmittedContext {
        SubmittedContext::from(ContextSpec::none())
    }

    fn refused() -> SubmittedBody<u8> {
        SubmittedBody::new(
            Err(ClassifiedReport::public(NgsiLdErrorType::BadRequestData, "undecodable").with_instance(uri())),
            uri(),
        )
    }

    /// The type and detail of a refusal.
    fn refusal(response: Result<impl Debug, ProblemResponse>) -> (NgsiLdErrorType, String) {
        let problem = response.expect_err("refused");
        (problem.problem().error_type(), problem.problem().detail().to_owned())
    }

    #[test]
    fn an_admitted_body_is_its_decoded_value_or_its_refusal() {
        let context = AdmittedContext::core_only();
        pretty_assertions::assert_eq!(SubmittedBody::new(Ok(7_u8), uri()).admitted(&context).ok(), Some(7));
        pretty_assertions::assert_eq!(
            refusal(refused().admitted(&context)),
            (NgsiLdErrorType::BadRequestData, "undecodable".to_owned())
        );
    }

    /// Clause 5.5.7 refuses an embedded `@context` before the payload rules
    /// are applied, so its refusal outranks the body's own.
    #[test]
    fn a_context_free_body_refuses_an_embedded_context_before_its_own_refusal() {
        let (error_type, detail) = refusal(refused().context_free(embedding()));
        pretty_assertions::assert_eq!(error_type, NgsiLdErrorType::BadRequestData);
        assert!(detail.contains("@context is embedded at /speed"), "{detail}");
        pretty_assertions::assert_eq!(refusal(refused().context_free(plain())).1, "undecodable");
        pretty_assertions::assert_eq!(SubmittedBody::new(Ok(7_u8), uri()).context_free(plain()).ok(), Some(7));
    }

    #[test]
    fn a_mapped_body_keeps_the_first_refusal() {
        let context = AdmittedContext::core_only();
        let mapped = SubmittedBody::new(Ok(7_u8), uri()).and_then(|value, _uri| Ok(u16::from(value) * 2));
        pretty_assertions::assert_eq!(mapped.admitted(&context).ok(), Some(14));
        let mapped = refused().and_then(|value, _uri| Ok(u16::from(value)));
        pretty_assertions::assert_eq!(refusal(mapped.admitted(&context)).1, "undecodable");
    }
}
