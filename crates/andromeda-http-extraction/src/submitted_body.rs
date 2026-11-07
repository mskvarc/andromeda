//! A request body admitted from its text, held until the user `@context` it
//! was submitted with is admitted, and decoded under that context.
//!
//! Clause 5.5.4 judges a payload "syntactically correct, according to the
//! @context", so the context is resolved before anything is judged under it:
//! a context that cannot be retrieved is clause 5.5.6's
//! `LdContextNotAvailable`, and clause 5.5.7 refuses a scoped or an embedded
//! user `@context` before the payload's own rules are applied. The payload is
//! read under that context too: a member written `@id` or `@type` (clause
//! 4.4), or under a term the context defines for a core term (clause 5.5.7),
//! is the core term it stands for. The extractor therefore only admits the
//! text, and the body is decoded once, when its [`AdmittedContext`] is handed
//! over, on the blocking pool when it is large. An operation that never
//! resolves its context decodes the body under the clause 4.4 keyword
//! spellings alone, once the context was found to embed nothing.

use crate::blocking_decode::{BlockingDecodePolicy, decode_payload};
use andromeda_http_response::{classified_report::ClassifiedReport, problem_response::ProblemResponse};
use andromeda_jsonld::user_context_admission::{AdmittedContext, SubmittedContext};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::attribute::{member_reading::MemberReading, written_attribute::WrittenAttribute};
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_ngsi_ld_representation::{compact_node::context_reading::ContextReading, context_terms::ContextTerms};
use axum::{body::Bytes, http::Uri};
use std::{borrow::Borrow, sync::Arc};

/// How a body is read from its admitted text under the terms of its context,
/// for the request it names.
type BodyReading<T> = Box<dyn FnOnce(&[u8], ContextReading<'_>, &Uri) -> Result<T, ProblemResponse> + Send>;

/// A request body admitted as text, read as a `T` once its context is.
#[must_use = "a submitted body is read once its context is admitted"]
pub struct SubmittedBody<T> {
    /// The admitted text of the body.
    text: Bytes,
    /// The request URI, the `instance` of every refusal of the body.
    uri: Uri,
    /// How the body is decoded beside the executor, when the stack published
    /// it.
    blocking_decode: Option<BlockingDecodePolicy>,
    /// How the body is read from its text.
    read: BodyReading<T>,
}

impl<T: Send + 'static> SubmittedBody<T> {
    /// The body `text` of the request `uri`, read with `read` once its context
    /// is admitted.
    pub(crate) fn new(
        text: Bytes,
        uri: Uri,
        blocking_decode: Option<BlockingDecodePolicy>,
        read: impl FnOnce(&[u8], ContextReading<'_>, &Uri) -> Result<T, ProblemResponse> + Send + 'static,
    ) -> SubmittedBody<T> {
        SubmittedBody {
            text,
            uri,
            blocking_decode,
            read: Box::new(read),
        }
    }

    /// The body read under `context`, which the operation admitted first.
    ///
    /// # Errors
    ///
    /// Returns the refusal the body's payload rules gave it, and an
    /// `InternalError` when the blocking decoding task fails.
    pub async fn admitted(self, context: &AdmittedContext) -> Result<T, ProblemResponse> {
        self.decoded(Arc::clone(context.terms())).await
    }

    /// The body of an operation that never resolves `context`, refused first
    /// when the body embedded a `@context` below its top level (clause 5.5.7),
    /// and read under the clause 4.4 keyword spellings alone.
    ///
    /// # Errors
    ///
    /// Returns a `BadRequestData` locating the embedded `@context`, then the
    /// refusal the body's payload rules gave it.
    pub async fn context_free(self, context: SubmittedContext) -> Result<T, ProblemResponse> {
        if let Err(embedded) = context.unused() {
            return Err(ClassifiedReport::public(NgsiLdErrorType::BadRequestData, embedded.to_string()).with_instance(self.uri));
        }
        self.decoded(ContextTerms::keyword_spellings()).await
    }

    /// The body `map` makes of the decoded one, refused as `map` refuses it.
    pub(crate) fn and_then<U: Send + 'static>(self, map: impl FnOnce(T, &Uri) -> Result<U, ProblemResponse> + Send + 'static) -> SubmittedBody<U> {
        let SubmittedBody {
            text,
            uri,
            blocking_decode,
            read,
        } = self;
        SubmittedBody::new(text, uri, blocking_decode, move |text, reading, uri| {
            read(text, reading, uri).and_then(|value| map(value, uri))
        })
    }

    /// Decode the body through `terms`, on the blocking pool when it is
    /// large.
    async fn decoded(self, terms: impl Borrow<ContextTerms> + Send + 'static) -> Result<T, ProblemResponse> {
        let SubmittedBody {
            text,
            uri,
            blocking_decode,
            read,
        } = self;
        let instance = uri.clone(); // clone: a failed decoding task still names the request
        decode_payload(blocking_decode.as_ref(), text.len(), move || read(&text, terms.borrow().reading(), &uri))
            .await
            .map_err(|failure| failure.report().with_instance(instance))?
    }
}

impl SubmittedBody<WrittenAttribute> {
    /// The member the bare Attribute Fragment of clauses 5.6.4 and 5.6.19 is,
    /// read under `context` for the attribute name `attribute` the request
    /// path carries.
    ///
    /// # Errors
    ///
    /// Returns the refusal of [`SubmittedBody::admitted`], and a
    /// `BadRequestData` for a fragment no clause admits where an attribute
    /// stands (clause 5.5.4).
    pub async fn admitted_attribute<M: MemberReading>(self, attribute: &AttributeName, context: &AdmittedContext) -> Result<M, ProblemResponse> {
        let instance = self.uri.clone(); // clone: the refusal of the fragment names the request the decode consumes
        let written = self.admitted(context).await?;
        written
            .read(attribute, context.terms().reading())
            .map_err(|refused| ClassifiedReport::public(NgsiLdErrorType::BadRequestData, refused.to_string()).with_instance(instance))
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
    use axum::{body::Bytes, http::Uri};
    use std::{borrow::Cow, fmt::Debug, str};

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

    /// A body read as the number it holds.
    fn decodable() -> SubmittedBody<u8> {
        SubmittedBody::new(Bytes::from_static(b"7"), uri(), None, |text, _reading, _uri| {
            Ok(str::from_utf8(text).ok().and_then(|digits| digits.parse().ok()).unwrap_or_default())
        })
    }

    fn refused() -> SubmittedBody<u8> {
        SubmittedBody::new(Bytes::from_static(b"x"), uri(), None, |_text, _reading, uri| {
            Err(ClassifiedReport::public(NgsiLdErrorType::BadRequestData, "undecodable").with_instance(uri.clone()))
        })
    }

    /// A body read as the core term the name it holds stands for.
    fn spelled(name: &'static str) -> SubmittedBody<String> {
        SubmittedBody::new(Bytes::from_static(name.as_bytes()), uri(), None, |text, reading, _uri| {
            Ok(reading
                .aliases()
                .core_name(Cow::Borrowed(str::from_utf8(text).unwrap_or_default()))
                .into_owned())
        })
    }

    /// The type and detail of a refusal.
    fn refusal(response: Result<impl Debug, ProblemResponse>) -> (NgsiLdErrorType, String) {
        let problem = response.expect_err("refused");
        (problem.problem().error_type(), problem.problem().detail().to_owned())
    }

    #[tokio::test]
    async fn an_admitted_body_is_its_decoded_value_or_its_refusal() {
        let context = AdmittedContext::core_only();
        pretty_assertions::assert_eq!(decodable().admitted(&context).await.ok(), Some(7));
        pretty_assertions::assert_eq!(
            refusal(refused().admitted(&context).await),
            (NgsiLdErrorType::BadRequestData, "undecodable".to_owned())
        );
    }

    /// Clause 4.4: the keyword spellings are read as the core terms whether
    /// or not the operation resolves its context.
    #[tokio::test]
    async fn a_body_is_read_through_the_aliases_of_its_context() {
        pretty_assertions::assert_eq!(spelled("@type").admitted(&AdmittedContext::core_only()).await.ok().as_deref(), Some("type"));
        pretty_assertions::assert_eq!(spelled("@id").context_free(plain()).await.ok().as_deref(), Some("id"));
        pretty_assertions::assert_eq!(spelled("speed").context_free(plain()).await.ok().as_deref(), Some("speed"));
    }

    /// Clause 5.5.7 refuses an embedded `@context` before the payload rules
    /// are applied, so its refusal outranks the body's own.
    #[tokio::test]
    async fn a_context_free_body_refuses_an_embedded_context_before_its_own_refusal() {
        let (error_type, detail) = refusal(refused().context_free(embedding()).await);
        pretty_assertions::assert_eq!(error_type, NgsiLdErrorType::BadRequestData);
        assert!(detail.contains("@context is embedded at /speed"), "{detail}");
        pretty_assertions::assert_eq!(refusal(refused().context_free(plain()).await).1, "undecodable");
        pretty_assertions::assert_eq!(decodable().context_free(plain()).await.ok(), Some(7));
    }

    #[tokio::test]
    async fn a_mapped_body_keeps_the_first_refusal() {
        let context = AdmittedContext::core_only();
        let mapped = decodable().and_then(|value, _uri| Ok(u16::from(value) * 2));
        pretty_assertions::assert_eq!(mapped.admitted(&context).await.ok(), Some(14));
        let mapped = refused().and_then(|value, _uri| Ok(u16::from(value)));
        pretty_assertions::assert_eq!(refusal(mapped.admitted(&context).await).1, "undecodable");
    }
}
