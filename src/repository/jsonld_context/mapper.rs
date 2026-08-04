//! Row-to-domain mapping for the JSON-LD context store.

use crate::repository::jsonld_context::rows::ContextMetadataRow;
use andromeda_domain::{
    jsonld_context::{ContextKind, ContextMetadata},
    uri::Uri,
};
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
    /// `number_of_hits` column was negative.
    #[error("negative number_of_hits in database: {value}")]
    NegativeHits {
        /// Raw database value.
        value: i64,
    },
    /// The `kind` column held a label no [`ContextKind`] variant covers.
    #[error("unknown jsonld_context_kind in database: {value}")]
    Kind {
        /// Raw database value.
        value: String,
    },
}

/// Convert a detailed-projection row into the domain [`ContextMetadata`].
pub fn row_to_metadata(row: ContextMetadataRow) -> Result<ContextMetadata, ContextMapperError> {
    let local_id: Uri = row.local_id.parse().map_err(|_| ContextMapperError::LocalId { value: row.local_id.clone() })?;

    let url = row
        .url
        .as_deref()
        .map(Url::parse)
        .transpose()
        .map_err(|source| ContextMapperError::Url { source })?;

    let number_of_hits = u64::try_from(row.number_of_hits).map_err(|_| ContextMapperError::NegativeHits { value: row.number_of_hits })?;

    let kind: ContextKind = row.kind.parse().map_err(|_| ContextMapperError::Kind { value: row.kind.clone() })?;

    Ok(ContextMetadata {
        url,
        local_id,
        kind,
        created_at: row.created_at,
        expires_at: row.expires_at,
        last_usage: row.last_usage,
        number_of_hits,
        extra_info: row.extra_info,
    })
}
