//! Row-to-domain mapping for the JSON-LD context store.

use crate::jsonld_context::{
    kind_code::kind_of,
    rows::{ContextMetadataRow, ContextSummaryRow},
};
use andromeda_jsonld::context_cache::StoredDocumentSource;
use andromeda_ngsi_ld_lexicon::identifier::{Identifier, IdentifierParseError};
use andromeda_ngsi_ld_model::jsonld_context::ContextMetadata;
use andromeda_postgresql_codes::{jsonld_context_kind::JsonldContextKindCode, registry::UnknownCompactCode};
use iri_rs::{InvalidIri, IriBuf};
use thiserror::Error;
use url::Url;

/// Errors produced while mapping a context row to the domain type.
#[derive(Debug, Error)]
pub enum ContextMapperError {
    /// Stored local identifier is not a valid URI.
    #[error("a stored local context identifier is not a valid URI: {0}")]
    LocalId(#[source] IdentifierParseError),
    /// Stored URL column is not a valid URL.
    #[error("invalid URL in database: {source}")]
    Url {
        /// Underlying parse error.
        #[source]
        source: url::ParseError,
    },
    /// Stored URL column is not a URI, or is longer than an identifier may be.
    #[error("URL in database is not an identifier: {0}")]
    UrlIdentifier(#[from] IdentifierParseError),
    /// `number_of_hits` column was negative.
    #[error("negative number_of_hits in database: {value}")]
    NegativeHits {
        /// Raw database value.
        value: i64,
    },
    /// The `kind` column held a number the kind registry never assigned.
    #[error("unreadable context kind in database: {0}")]
    Kind(#[from] UnknownCompactCode),
    /// Stored serving URL is not an IRI reference the hit counter can key on.
    #[error("a stored serving URL is not an IRI: {0}")]
    ServingIri(#[source] InvalidIri<String>),
    /// Stored document URL is not an IRI a document can be based on.
    #[error("a stored document URL is not an IRI: {0}")]
    DocumentUrl(#[source] InvalidIri<String>),
    /// A `Cached` row stores no URL its document was read from.
    #[error("a cached context row stores no document URL")]
    CachedWithoutDocumentUrl,
    /// A row the broker stores itself claims a URL its document was read from.
    #[error("a hosted or implicitly created context row stores a document URL")]
    DocumentUrlOnStoredContext,
}

/// Where the document of a row of `kind` came from, given the URL the row says
/// it was finally read from.
///
/// A `Cached` row is a download and stores that URL; a Hosted or
/// `ImplicitlyCreated` row is a document the broker stores itself and stores
/// none. The table's check holds both, so a row breaking either is refused as
/// undecodable rather than read with a guessed base.
pub(crate) fn row_document_source(kind: i16, document_url: Option<String>) -> Result<StoredDocumentSource, ContextMapperError> {
    match (JsonldContextKindCode::try_from(kind)?, document_url) {
        (JsonldContextKindCode::Cached, Some(document_url)) => Ok(StoredDocumentSource::Downloaded {
            document_url: IriBuf::new(document_url).map_err(ContextMapperError::DocumentUrl)?,
        }),
        (JsonldContextKindCode::Cached, None) => Err(ContextMapperError::CachedWithoutDocumentUrl),
        (JsonldContextKindCode::Hosted | JsonldContextKindCode::ImplicitlyCreated, None) => Ok(StoredDocumentSource::Broker),
        (JsonldContextKindCode::Hosted | JsonldContextKindCode::ImplicitlyCreated, Some(_)) => Err(ContextMapperError::DocumentUrlOnStoredContext),
    }
}

/// Convert a detailed-projection row into the domain [`ContextMetadata`].
pub(crate) fn row_to_metadata(row: &ContextMetadataRow) -> Result<ContextMetadata, ContextMapperError> {
    let local_id: Identifier = row.local_id.parse().map_err(ContextMapperError::LocalId)?;

    let url = row
        .url
        .as_deref()
        .map(Url::parse)
        .transpose()
        .map_err(|source| ContextMapperError::Url { source })?;

    // Kept verbatim, not re-serialized from `url`: the hit counter is keyed on
    // this exact stored string (clause 5.13.3.5), so the `numberOfHits` fold
    // must key on the same bytes.
    let serving_iri = row
        .url
        .as_deref()
        .map(|raw| IriBuf::new(raw.to_owned()).map_err(ContextMapperError::ServingIri))
        .transpose()?;

    let number_of_hits = u64::try_from(row.number_of_hits).map_err(|_| ContextMapperError::NegativeHits { value: row.number_of_hits })?;

    let kind = kind_of(JsonldContextKindCode::try_from(row.kind)?);

    Ok(ContextMetadata::builder()
        .url(url)
        .serving_iri(serving_iri)
        .local_id(local_id)
        .kind(kind)
        .created_at(row.created_at)
        .expires_at(row.expires_at)
        .last_usage(row.last_usage)
        .number_of_hits(number_of_hits)
        .build())
}

/// Convert a summary projection into the public URL when present, otherwise its local identifier.
pub(crate) fn row_to_identifier(row: ContextSummaryRow) -> Result<Identifier, ContextMapperError> {
    match row.url {
        Some(raw) => Ok(Identifier::try_from(Url::parse(&raw).map_err(|source| ContextMapperError::Url { source })?)?),
        None => row.local_id.parse().map_err(ContextMapperError::LocalId),
    }
}

/// The local identifiers of the first and last of `rows`, read in
/// presentation order, or `None` for an empty page.
pub(crate) fn page_boundary<T>(rows: &[T], local_id: impl Fn(&T) -> &str) -> Result<Option<(Identifier, Identifier)>, ContextMapperError> {
    match (rows.first(), rows.last()) {
        (Some(first), Some(last)) => Ok(Some((
            local_id(first).parse().map_err(ContextMapperError::LocalId)?,
            local_id(last).parse().map_err(ContextMapperError::LocalId)?,
        ))),
        (None, _) | (_, None) => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use crate::jsonld_context::mapper::{ContextMapperError, page_boundary, row_document_source};
    use andromeda_jsonld::context_cache::StoredDocumentSource;
    use andromeda_postgresql_codes::jsonld_context_kind::JsonldContextKindCode;
    use iri_rs::IriBuf;

    /// A download reads back with the URL it was finally read from, which is
    /// the base its relative references resolve against.
    #[test]
    fn a_cached_row_reads_back_as_a_download_from_its_document_url() {
        let source = row_document_source(JsonldContextKindCode::Cached.code(), Some("https://mirror.example.com/v2/c.jsonld".to_owned()));

        pretty_assertions::assert_eq!(
            source.expect("a readable cached row"),
            StoredDocumentSource::Downloaded {
                document_url: IriBuf::new("https://mirror.example.com/v2/c.jsonld".to_owned()).expect("a valid IRI"),
            }
        );
    }

    /// A document the broker stores itself was never read from anywhere.
    #[test]
    fn a_hosted_or_implicitly_created_row_reads_back_as_the_brokers_own() {
        for kind in [JsonldContextKindCode::Hosted, JsonldContextKindCode::ImplicitlyCreated] {
            pretty_assertions::assert_eq!(
                row_document_source(kind.code(), None).expect("a readable stored row"),
                StoredDocumentSource::Broker,
                "{kind:?}"
            );
        }
    }

    /// The table's check ties the document URL to the `Cached` kind; a row
    /// breaking it is refused rather than read with a guessed base.
    #[test]
    fn a_row_breaking_the_document_url_invariant_is_refused() {
        assert!(matches!(
            row_document_source(JsonldContextKindCode::Cached.code(), None),
            Err(ContextMapperError::CachedWithoutDocumentUrl)
        ));
        assert!(matches!(
            row_document_source(JsonldContextKindCode::Hosted.code(), Some("https://example.com/c.jsonld".to_owned())),
            Err(ContextMapperError::DocumentUrlOnStoredContext)
        ));
    }

    #[test]
    fn a_document_url_that_is_not_an_iri_is_refused() {
        assert!(matches!(
            row_document_source(JsonldContextKindCode::Cached.code(), Some("not an iri".to_owned())),
            Err(ContextMapperError::DocumentUrl(_))
        ));
    }

    /// The cursors of a page are built from its first and last local
    /// identifiers.
    #[test]
    fn a_page_is_bounded_by_its_first_and_last_local_identifier() {
        let boundary = page_boundary(&["urn:a", "urn:b", "urn:c"], |id| id).expect("readable identifiers");
        pretty_assertions::assert_eq!(
            boundary.map(|(first, last)| (first.to_string(), last.to_string())),
            Some(("urn:a".to_owned(), "urn:c".to_owned()))
        );
    }

    #[test]
    fn an_empty_page_has_no_boundary() {
        assert!(page_boundary::<&str>(&[], |id| id).expect("nothing to read").is_none());
    }

    #[test]
    fn an_unknown_kind_is_refused() {
        assert!(matches!(row_document_source(0, None), Err(ContextMapperError::Kind(_))));
    }
}
