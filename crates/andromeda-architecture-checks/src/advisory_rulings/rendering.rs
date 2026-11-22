//! The VEX document generated from the rulings and the reasons `deny.toml`
//! gives for ignoring their advisories.
//!
//! An advisory gets one statement under each of its identifiers, because some
//! scanners apply only a statement named by the identifier they report, and
//! each statement lists the others as aliases. Every statement names both
//! forms of the broker image, with the ruling's crate versions inside, and
//! repeats the ignore's reason as its impact statement.
//!
//! A statement keeps the timestamp the checked-in document gives it while it
//! reads the same, and takes the time of generation once it changes. The
//! document's version is raised and its last update moved whenever a statement
//! or the document's identifier changes.

use crate::advisory_rulings::{
    broker_image::{BrokerImage, ImageForm},
    rulings_file::{CrateVersion, Ruling},
    source_repository::SourceRepository,
    vex_document::{Product, Statement, Status, Subcomponent, VexDocument, Vulnerability},
};
use jiff::Timestamp;
use packageurl::PackageUrl;
use std::collections::BTreeMap;
use strum::IntoEnumIterator;
use thiserror::Error;
use url::Url;

/// Who issues the rulings.
const AUTHOR: &str = "Andromeda";

/// The package URL type of a crate.
const CRATE_TYPE: &str = "cargo";

/// Why the document cannot be generated.
#[derive(Debug, Error)]
pub(crate) enum GenerationError {
    /// The repository URL takes no path, so the document has no identifier.
    #[error("the repository the root manifest names takes no path, so the document has no identifier")]
    NoDocumentId,
    /// The clock reads an instant the document cannot carry.
    #[error("the clock reads an instant the document cannot carry")]
    Clock(#[from] jiff::Error),
    /// A crate version cannot be spelt as a package URL.
    #[error("a crate version cannot be spelt as a package URL")]
    PackageUrl(#[from] packageurl::Error),
    /// The document cannot be written as JSON.
    #[error("the document cannot be written as JSON")]
    Json(#[from] serde_json::Error),
}

/// The text of the VEX document at the workspace-relative `path` of `source`
/// that states `rulings` with the reasons `reasons` records. `checked_in` is
/// the document's current text, whose timestamps and version carry over where
/// nothing changed; a text that is absent or no OpenVEX document carries
/// nothing over.
///
/// # Errors
///
/// Fails when the document has no identifier, the clock reads no instant it
/// can carry, a crate version is no package URL, or the JSON cannot be written.
pub(crate) fn document_text(
    rulings: &BTreeMap<String, Ruling>,
    reasons: &BTreeMap<String, Option<String>>,
    source: &SourceRepository,
    path: &str,
    checked_in: Option<&str>,
) -> Result<String, GenerationError> {
    let id = source.file_on_default_branch(path).ok_or(GenerationError::NoDocumentId)?;
    let existing = checked_in.and_then(|text| serde_json::from_str::<VexDocument>(text).ok());
    let now = Timestamp::from_second(Timestamp::now().as_second())?;
    let document = vex_document(rulings, reasons, &BrokerImage::published_by(source), id, existing.as_ref(), now)?;
    Ok(serde_json::to_string_pretty(&document)? + "\n")
}

/// The OpenVEX context the document follows.
fn openvex_context() -> Url {
    url_macro::url!("https://openvex.dev/ns/v0.2.0")
}

/// The VEX document published at `id` that states each of `rulings` with the
/// reason `reasons` records for its advisory, about every digest of `image`.
/// `existing` is the checked-in document, whose timestamps and version carry
/// over where nothing changed, and `now` the time a change is dated. A ruling
/// whose advisory has no reason is left out.
///
/// # Errors
///
/// Fails when a crate version cannot be spelt as a package URL.
pub(crate) fn vex_document(
    rulings: &BTreeMap<String, Ruling>,
    reasons: &BTreeMap<String, Option<String>>,
    image: &BrokerImage,
    id: Url,
    existing: Option<&VexDocument>,
    now: Timestamp,
) -> Result<VexDocument, packageurl::Error> {
    let previous: BTreeMap<&str, &Statement> = existing
        .into_iter()
        .flat_map(|document| &document.statements)
        .map(|statement| (statement.vulnerability.name.as_str(), statement))
        .collect();
    let mut statements = Vec::new();
    for (advisory, ruling) in rulings {
        let Some(reason) = reasons.get(advisory).and_then(Option::as_deref) else {
            continue;
        };
        let subcomponents = ruling.crates.iter().map(crate_purl).collect::<Result<Vec<_>, _>>()?;
        for name in &ruling.identifiers {
            let statement = Statement {
                vulnerability: Vulnerability {
                    name: name.clone(),
                    aliases: ruling.identifiers.iter().filter(|other| *other != name).cloned().collect(),
                },
                timestamp: now,
                products: ImageForm::iter()
                    .map(|form| Product {
                        id: image.purl(form).to_owned(),
                        subcomponents: subcomponents.iter().map(|id| Subcomponent { id: id.clone() }).collect(),
                    })
                    .collect(),
                status: Status::NotAffected,
                justification: ruling.justification,
                impact: reason.to_owned(),
            };
            statements.push(dated(statement, previous.get(name.as_str()).copied(), now));
        }
    }
    let unchanged = existing.filter(|document| document.id == id && document.statements == statements);
    let version = match (unchanged, existing) {
        (Some(document), _) => document.version,
        (None, Some(document)) => document.version.saturating_add(1),
        (None, None) => 1,
    };
    Ok(VexDocument {
        context: openvex_context(),
        id,
        author: AUTHOR.to_owned(),
        timestamp: existing.map_or(now, |document| document.timestamp),
        last_updated: unchanged.map_or(now, |document| document.last_updated),
        version,
        statements,
    })
}

/// `statement` with the timestamp of `previous`, the checked-in statement of
/// the same name, when that one reads the same apart from its timestamp, and
/// with `now` otherwise.
fn dated(mut statement: Statement, previous: Option<&Statement>, now: Timestamp) -> Statement {
    if let Some(previous) = previous {
        statement.timestamp = previous.timestamp;
        if *previous != statement {
            statement.timestamp = now;
        }
    }
    statement
}

/// The package URL of `crate_version`.
fn crate_purl(crate_version: &CrateVersion) -> Result<String, packageurl::Error> {
    let mut purl = PackageUrl::new(CRATE_TYPE, crate_version.name.as_str())?;
    purl.with_version(crate_version.version.as_str())?;
    Ok(purl.to_string())
}
