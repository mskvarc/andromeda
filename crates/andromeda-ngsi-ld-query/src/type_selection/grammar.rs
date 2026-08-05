//! Abstract syntax of the entity type selection language (clause 4.17).
//!
//! Two shapes of the same tree: [`TypeExprRaw`] carries the compact names the
//! client wrote, [`TypeExpr`] carries the IRIs they expand to.

use iri_rs::IriBuf;

/// Raw (pre-expansion) type expression AST node.
///
/// Leaf nodes contain compact type names that must be expanded to full IRIs
/// via the JSON-LD context before use in queries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeExprRaw {
    /// A single compact type name.
    Type(String),
    /// All children must match (semicolon-separated).
    And(Vec<TypeExprRaw>),
    /// Any child may match (comma/pipe-separated).
    Or(Vec<TypeExprRaw>),
}

/// Boolean expression over entity type IRIs (NGSI-LD S4.17).
///
/// Supports AND (`And`), OR (`Or`), and single type (`Type`) nodes.
/// Constructed by parsing the `type` query parameter's selection language
/// grammar and expanding compact type names to full IRIs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeExpr {
    /// Matches entities whose types include this specific IRI.
    Type(IriBuf),
    /// Matches entities whose types satisfy all children (semicolon-separated).
    And(Vec<TypeExpr>),
    /// Matches entities whose types satisfy any child (comma/pipe-separated).
    Or(Vec<TypeExpr>),
}
