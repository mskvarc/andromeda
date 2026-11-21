//! The part of an OpenVEX v0.2.0 document the gate reads.
//!
//! Members the gate does not compare, the document's identity, author,
//! timestamp and version and a vulnerability's aliases, are read past. The
//! status and justification tokens are the ones the OpenVEX specification
//! defines, so a misspelt token fails to parse.

use serde::Deserialize;
use strum::Display;

/// An OpenVEX document, read for its statements.
#[derive(Debug, Deserialize)]
pub(crate) struct VexDocument {
    /// The statements, each a ruling on one vulnerability.
    pub(crate) statements: Vec<Statement>,
}

/// One ruling on one vulnerability in the named products.
#[derive(Debug, Deserialize)]
pub(crate) struct Statement {
    /// The vulnerability ruled on.
    pub(crate) vulnerability: Vulnerability,
    /// The products the ruling covers.
    pub(crate) products: Vec<Product>,
    /// How the vulnerability affects the products.
    pub(crate) status: Status,
    /// Why the products are not affected; `None` when the statement gives no
    /// justification.
    pub(crate) justification: Option<Justification>,
    /// The ruling in prose; `None` when the statement gives none.
    #[serde(rename = "impact_statement")]
    pub(crate) impact: Option<String>,
}

/// The vulnerability a statement rules on.
#[derive(Debug, Deserialize)]
pub(crate) struct Vulnerability {
    /// The advisory identifier the vulnerability is named by, such as a
    /// `RUSTSEC-` id.
    pub(crate) name: String,
}

/// A product a statement covers, with the components inside it that carry the
/// vulnerability.
#[derive(Debug, Deserialize)]
pub(crate) struct Product {
    /// The product's package URL.
    #[serde(rename = "@id")]
    pub(crate) id: String,
    /// The components inside the product the ruling is about; empty when the
    /// statement names none.
    #[serde(default)]
    pub(crate) subcomponents: Vec<Subcomponent>,
}

/// A component inside a product.
#[derive(Debug, Deserialize)]
pub(crate) struct Subcomponent {
    /// The component's package URL.
    #[serde(rename = "@id")]
    pub(crate) id: String,
}

/// How a vulnerability affects a product, as the OpenVEX specification spells
/// the status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub(crate) enum Status {
    /// No remediation is needed.
    NotAffected,
    /// The product carries the vulnerability.
    Affected,
    /// The product no longer carries the vulnerability.
    Fixed,
    /// Whether the product is affected is not settled yet.
    UnderInvestigation,
}

/// Why a product is not affected, as the OpenVEX specification spells the
/// justification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
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
