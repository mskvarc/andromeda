//! A paged query a client sent by POST, held so the page links of its result
//! can name it.
//!
//! Every page link is a URI-reference a client dereferences with GET (clause
//! 6.3.10). A query sent by POST travels in the request body (clauses 6.23.3.1
//! and 6.24.3.1), which no GET carries, and the POST binding exists because a
//! query can outgrow a URL (clause 6.23.1), so a link cannot carry the query
//! either. The broker holds the query instead, and a link names it by a digest:
//! an opaque pointer, as clause 5.5.9.2 allows.
//!
//! The digest covers the query, the API it was posted to and the `@context` it
//! was read under. The same query posted again is named alike, and a query
//! read under another context, or posted to the other API, is another query.

use crate::error::PostedQueryError;
use andromeda_jsonld::{context_spec::ContextSpec, request_context::RequestContext};
use andromeda_ngsi_ld_model::query::body::Query;
use andromeda_ngsi_ld_request::pagination::query_digest::QueryDigest;
use strum::{Display, EnumIter, IntoStaticStr};

/// The API a query was posted to, and whose GET binding its page links follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display, EnumIter, IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub enum PostedQueryKind {
    /// Query Entities, posted as `POST /entityOperations/query` (clause
    /// 6.23.3.1) and followed as `GET /entities`.
    Entities,
    /// Query Temporal Evolution of Entities, posted as
    /// `POST /temporal/entityOperations/query` (clause 6.24.3.1) and followed
    /// as `GET /temporal/entities`.
    Temporal,
}

/// A clause 5.2.23 Query sent by POST for a paged result, with the `@context`
/// it was read under, named by the digest of both and of its API.
#[derive(Debug, Clone, PartialEq)]
pub struct PostedQuery {
    /// The digest naming the query, derived from every other field.
    digest: QueryDigest,
    /// The API the query was posted to.
    kind: PostedQueryKind,
    /// The query as the broker read it, without its `@context`.
    query: Query,
    /// The context the request named, with the core context appended.
    context: ContextSpec,
}

impl PostedQuery {
    /// The query `query` posted to `kind`, read under the context the request
    /// named, `context`.
    ///
    /// The digest is taken over three parts, the API, the query's JSON and the
    /// resolved `@context`: an odd number of parts, which no query string's
    /// digest is taken over, so the digest of a posted query never names a query
    /// string.
    ///
    /// # Errors
    ///
    /// [`PostedQueryError::Unencodable`] when the query cannot be written as
    /// JSON.
    pub fn new(kind: PostedQueryKind, query: Query, context: RequestContext) -> Result<PostedQuery, PostedQueryError> {
        let context = ContextSpec::from_request(context);
        let document = serde_json::to_vec(&query).map_err(PostedQueryError::Unencodable)?;
        let api: &'static str = kind.into();
        let digest = QueryDigest::of_parts([api.as_bytes(), document.as_slice(), context.context_key().as_bytes()]);
        Ok(PostedQuery { digest, kind, query, context })
    }

    /// The digest a page link names the query by.
    #[must_use]
    pub const fn digest(&self) -> &QueryDigest {
        &self.digest
    }

    /// The API the query was posted to.
    #[must_use]
    pub const fn kind(&self) -> PostedQueryKind {
        self.kind
    }

    /// The query as the broker read it, without its `@context`.
    #[must_use]
    pub const fn query(&self) -> &Query {
        &self.query
    }

    /// The context the query is read under.
    #[must_use]
    pub const fn context(&self) -> &ContextSpec {
        &self.context
    }

    /// The query and the context it is read under.
    #[must_use]
    pub fn into_parts(self) -> (Query, ContextSpec) {
        (self.query, self.context)
    }

    /// Refuse a request following a pointer of this query that names another
    /// `@context` than the one the query was posted with (clause 5.5.9.1).
    ///
    /// The query is read under the context it was posted with, so a follower
    /// naming none reads it under that one. A follower naming one names a
    /// context that resolves to the same `@context`, or is refused.
    ///
    /// # Errors
    ///
    /// [`PostedQueryError::ContextChanged`] for a follower naming another
    /// context.
    pub fn admit_follower(&self, follower: &ContextSpec) -> Result<(), PostedQueryError> {
        match follower.request_context() {
            RequestContext::Default => Ok(()),
            RequestContext::Link(_) | RequestContext::Body(_) if follower.context_key() == self.context.context_key() => Ok(()),
            RequestContext::Link(_) | RequestContext::Body(_) => Err(PostedQueryError::ContextChanged),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::PostedQueryError,
        posted_query::{PostedQuery, PostedQueryKind},
    };
    use andromeda_json_literal::JsonLiteral;
    use andromeda_jsonld::{context_spec::ContextSpec, request_context::RequestContext};
    use andromeda_ngsi_ld_model::query::body::Query;
    use andromeda_ngsi_ld_request::pagination::query_digest::QueryDigest;
    use iri_rs::IriBuf;
    use pretty_assertions::{assert_eq, assert_ne};

    /// Two selectors with different `idPattern`s: a query no query string can
    /// spell.
    const UNION: &str =
        r#"{"type":"Query","entities":[{"idPattern":"urn:ngsi-ld:A:.*","type":"Vehicle"},{"idPattern":"urn:ngsi-ld:B:.*","type":"Vehicle"}],"q":"speed>10"}"#;

    fn query(text: &str) -> Query {
        serde_json::from_str(text).expect("a clause 5.2.23 Query")
    }

    fn link(url: &str) -> RequestContext {
        RequestContext::Link(IriBuf::new(url.to_owned()).expect("a valid IRI"))
    }

    fn posted(kind: PostedQueryKind, text: &str, context: RequestContext) -> PostedQuery {
        PostedQuery::new(kind, query(text), context).expect("serialises")
    }

    #[test]
    fn the_same_query_posted_again_is_named_alike() {
        assert_eq!(
            posted(PostedQueryKind::Entities, UNION, RequestContext::Default).digest(),
            posted(PostedQueryKind::Entities, UNION, RequestContext::Default).digest()
        );
    }

    #[test]
    fn another_api_context_or_member_names_another_query() {
        let named = posted(PostedQueryKind::Entities, UNION, RequestContext::Default);
        let body = RequestContext::Body(JsonLiteral::admit(r#"{"speed":"https://example.com/speed"}"#).expect("a context"));
        for other in [
            posted(PostedQueryKind::Temporal, UNION, RequestContext::Default),
            posted(PostedQueryKind::Entities, UNION, link("https://example.com/a.jsonld")),
            posted(PostedQueryKind::Entities, UNION, body),
            posted(PostedQueryKind::Entities, &UNION.replace("speed>10", "speed>11"), RequestContext::Default),
            posted(
                PostedQueryKind::Entities,
                &UNION.replace("urn:ngsi-ld:B:", "urn:ngsi-ld:C:"),
                RequestContext::Default,
            ),
        ] {
            assert_ne!(other.digest(), named.digest(), "{other:?}");
        }
    }

    /// The broker stores the query as its JSON and reads it back from that
    /// JSON, so the query read back is named as the one posted.
    #[test]
    fn a_query_read_back_from_its_json_is_named_as_the_one_posted() {
        let named = posted(PostedQueryKind::Temporal, UNION, link("https://example.com/a.jsonld"));
        let stored = serde_json::to_string(named.query()).expect("serialises");
        assert_eq!(posted(PostedQueryKind::Temporal, &stored, link("https://example.com/a.jsonld")), named);
    }

    #[test]
    fn a_posted_query_is_not_named_as_the_query_string_of_its_selection() {
        let named = posted(PostedQueryKind::Entities, r#"{"type":"Query","q":"speed>10"}"#, RequestContext::Default);
        assert_ne!(named.digest(), &QueryDigest::of(Some("q=speed%3E10")));
    }

    /// Clause 5.5.9.1: a follower naming no context reads the query under the
    /// one it was posted with; one naming a context has to name that one.
    #[test]
    fn a_follower_names_no_context_or_the_one_the_query_was_posted_with() {
        let linked = posted(PostedQueryKind::Entities, UNION, link("https://example.com/a.jsonld"));
        assert!(linked.admit_follower(&ContextSpec::none()).is_ok());
        assert!(
            linked
                .admit_follower(&ContextSpec::from_link(IriBuf::new("https://example.com/a.jsonld".to_owned()).expect("an IRI")))
                .is_ok()
        );
        let other = ContextSpec::from_link(IriBuf::new("https://example.com/b.jsonld".to_owned()).expect("an IRI"));
        assert!(matches!(linked.admit_follower(&other), Err(PostedQueryError::ContextChanged)));
        let bodied = posted(
            PostedQueryKind::Entities,
            UNION,
            RequestContext::Body(JsonLiteral::admit(r#"{"speed":"https://example.com/speed"}"#).expect("a context")),
        );
        assert!(bodied.admit_follower(&ContextSpec::none()).is_ok());
        assert!(matches!(bodied.admit_follower(&other), Err(PostedQueryError::ContextChanged)));
    }

    /// The core context resolves to the context a request naming none is
    /// read under, so naming it follows a query posted without a context.
    #[test]
    fn a_follower_naming_the_core_context_follows_a_query_posted_without_one() {
        let plain = posted(PostedQueryKind::Temporal, UNION, RequestContext::Default);
        let core = ContextSpec::from_link(IriBuf::from(ContextSpec::none().effective_core_url()));
        assert!(plain.admit_follower(&core).is_ok());
    }

    #[test]
    fn the_api_is_spelled_in_kebab_case() {
        assert_eq!(PostedQueryKind::Entities.to_string(), "entities");
        assert_eq!(PostedQueryKind::Temporal.to_string(), "temporal");
    }
}
