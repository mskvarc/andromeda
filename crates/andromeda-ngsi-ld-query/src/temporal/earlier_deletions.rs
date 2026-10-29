//! The request for the deletions of one Attribute stream a context source has
//! not yet been asked for (clauses 4.5.7 and 4.11).

use crate::dataset::DatasetSelector;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use chrono::{DateTime, Utc};

/// A repeated deletion request to one context source, for the streams of one
/// Attribute and `datasetId` of some Entities.
///
/// A source asked for the latest deletion before an instant answers one
/// deletion per Attribute, per Entity (Table 6.18.3.2-1). Clause 4.11 makes
/// `before` an exclusive bound on the chosen Temporal Property, so asking again
/// with `before` set to the `deletedAt` of the earliest deletion returned
/// reaches the next older one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EarlierDeletions {
    /// The registration of the only source asked.
    pub source: Identifier,
    /// The Entities whose stream is asked about, all of which answered a
    /// deletion at `before`.
    pub entities: Vec<Identifier>,
    /// The Attribute whose deletions are asked for.
    pub attribute: AttributeName,
    /// The `datasetId` whose deletions are asked for.
    pub dataset: DatasetSelector,
    /// The exclusive upper bound on `deletedAt` of the deletions asked for.
    pub before: DateTime<Utc>,
}
