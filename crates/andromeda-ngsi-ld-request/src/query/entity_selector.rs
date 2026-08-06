//! The per-selector form of a query's entity selection (clause 5.2.11).

/// One entity selector carried by a POST query body's `entities[]` array, still
/// in its raw pre-expansion form.
///
/// A GET query has no structured selectors — its flat `id`/`type`/`idPattern`
/// parameters describe one implicit selector — so this type exists to preserve
/// the per-selector grouping a POST body draws that a comma-joined parameter
/// cannot express: `{id:A,type:T1}` and `{id:B,type:T2}` are a union, not the
/// cross product `(A,T2)` that a flattened `id=A,B`/`type=T1,T2` would match.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct EntitySelectorParams {
    /// The selector's entity identifiers, comma-separated and raw (`id`).
    pub id: Option<String>,
    /// A regular expression the selector's identifiers must match (`idPattern`).
    pub id_pattern: Option<String>,
    /// The selector's raw type-selection expression (clause 4.17 `type`).
    pub type_: Option<String>,
}
