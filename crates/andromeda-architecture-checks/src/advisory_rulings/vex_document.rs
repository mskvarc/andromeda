//! The OpenVEX v0.2.0 document the gate generates, and reads back for the
//! timestamps and the version it carries over.
//!
//! Members are declared in the order the document lists them, so the rendered
//! text keeps one layout. The status and justification tokens are the ones the
//! OpenVEX specification defines, and every timestamp is an RFC 3339 instant.

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use strum::EnumString;
use url::Url;

/// An OpenVEX document.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct VexDocument {
    /// The OpenVEX context the document follows.
    #[serde(rename = "@context")]
    pub(crate) context: Url,
    /// Where the document is published.
    #[serde(rename = "@id")]
    pub(crate) id: Url,
    /// Who issues the rulings.
    pub(crate) author: String,
    /// When the document was first issued.
    pub(crate) timestamp: Timestamp,
    /// When a statement last changed.
    pub(crate) last_updated: Timestamp,
    /// The document's version, raised whenever a statement changes.
    pub(crate) version: u64,
    /// The statements, each a ruling on one vulnerability identifier.
    pub(crate) statements: Vec<Statement>,
}

/// One ruling on one vulnerability in the named products.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Statement {
    /// The vulnerability ruled on.
    pub(crate) vulnerability: Vulnerability,
    /// When the ruling was last known to hold, which a change elsewhere in the
    /// document leaves as it is.
    pub(crate) timestamp: Timestamp,
    /// The products the ruling covers.
    pub(crate) products: Vec<Product>,
    /// How the vulnerability affects the products.
    pub(crate) status: Status,
    /// Why the products are not affected.
    pub(crate) justification: Justification,
    /// The ruling in prose.
    #[serde(rename = "impact_statement")]
    pub(crate) impact: String,
}

/// The vulnerability a statement rules on.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Vulnerability {
    /// The identifier the statement is named by, such as a `RUSTSEC-`, `CVE-`
    /// or `GHSA-` id.
    pub(crate) name: String,
    /// The vulnerability's other identifiers; empty when it has none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) aliases: Vec<String>,
}

/// A product a statement covers, with the components inside it that carry the
/// vulnerability.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Product {
    /// The product's package URL.
    #[serde(rename = "@id")]
    pub(crate) id: String,
    /// The components inside the product the ruling is about.
    pub(crate) subcomponents: Vec<Subcomponent>,
}

/// A component inside a product.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Subcomponent {
    /// The component's package URL.
    #[serde(rename = "@id")]
    pub(crate) id: String,
}

/// How a vulnerability affects a product. An advisory `deny.toml` ignores
/// affects none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Status {
    /// No remediation is needed.
    NotAffected,
}

/// Why a product is not affected, as the OpenVEX specification spells the
/// justification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub(crate) enum Justification {
    /// The vulnerable component is not in the product.
    ComponentNotPresent,
    /// The component is present, its vulnerable code is not.
    VulnerableCodeNotPresent,
    /// The vulnerable code is present and never runs.
    VulnerableCodeNotInExecutePath,
    /// The vulnerable code runs, on input no adversary controls.
    VulnerableCodeCannotBeControlledByAdversary,
    /// The product mitigates the vulnerability itself.
    InlineMitigationsAlreadyExist,
}

impl Justification {
    /// Whether the ruling rests on what the crates linking the component call
    /// into it, so it holds only for the linking crates it was proven for.
    pub(crate) const fn rests_on_callers(self) -> bool {
        match self {
            Justification::VulnerableCodeNotInExecutePath | Justification::VulnerableCodeCannotBeControlledByAdversary => true,
            Justification::ComponentNotPresent | Justification::VulnerableCodeNotPresent | Justification::InlineMitigationsAlreadyExist => false,
        }
    }
}
