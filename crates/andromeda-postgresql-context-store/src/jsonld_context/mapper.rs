//! Row-to-domain mapping for the JSON-LD context store.

use crate::jsonld_context::{
    kind_code::kind_of,
    rows::{ContextMetadataRow, ContextSummaryRow},
};
use andromeda_ngsi_ld_lexicon::{identifier_length::IdentifierTooLong, uri::Uri};
use andromeda_ngsi_ld_model::jsonld_context::ContextMetadata;
use andromeda_postgresql_codes::{jsonld_context_kind::JsonldContextKindCode, registry::UnknownCompactCode};
use iri_rs::IriBuf;
use thiserror::Error;
use url::Url;

/// Errors produced while mapping a context row to the domain type.
#[derive(Debug, Error)]
pub enum ContextMapperError {
    /// Stored local identifier is not a valid URI.
    #[error("invalid URI in database: {value}")]
    LocalId {
        /// Raw database value.
        value: String,
    },
    /// Stored URL column is not a valid URL.
    #[error("invalid URL in database: {source}")]
    Url {
        /// Underlying parse error.
        #[source]
        source: url::ParseError,
    },
    /// Stored URL column is longer than an identifier may be.
    #[error("over-long URL in database: {0}")]
    UrlLength(#[from] IdentifierTooLong),
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
    #[error("invalid serving URL in database: {value}")]
    ServingIri {
        /// Raw database value.
        value: String,
    },
}

/// Convert a detailed-projection row into the domain [`ContextMetadata`].
pub(crate) fn row_to_metadata(row: ContextMetadataRow) -> Result<ContextMetadata, ContextMapperError> {
    let local_id: Uri = row.local_id.parse().map_err(|_| ContextMapperError::LocalId { value: row.local_id.clone() })?;

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
        .map(|raw| IriBuf::new(raw.to_owned()).map_err(|error| ContextMapperError::ServingIri { value: error.0 }))
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
        .extra_info(row.extra_info)
        .build())
}

/// Convert a summary projection into the public URL when present, otherwise its local identifier.
pub(crate) fn row_to_identifier(row: ContextSummaryRow) -> Result<Uri, ContextMapperError> {
    match row.url {
        Some(raw) => Ok(Uri::try_from(Url::parse(&raw).map_err(|source| ContextMapperError::Url { source })?)?),
        None => row.id.parse().map_err(|_| ContextMapperError::LocalId { value: row.id }),
    }
}
