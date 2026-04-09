//! JSON-LD expandable trait for type-safe document construction.
//!
//! Types implementing [`JsonLdExpandable`] can be converted to their expanded
//! JSON-LD representation (full IRIs for all properties). The expanded form
//! can then be compacted by the engine using a JSON-LD context.

use serde_json::Value;

/// A type that can produce an expanded JSON-LD representation.
///
/// The expanded form uses full IRI keys and `@value` / `@id` wrappers,
/// making it ready for compaction by a JSON-LD processor.
///
/// This trait is typically derived via the `JsonLdExpandable` derive macro
/// from the `json-ld-expandable` crate.
///
/// # Example
///
/// ```ignore
/// use json_ld_expandable::JsonLdExpandable;
///
/// #[derive(JsonLdExpandable)]
/// #[jsonld(
///     type = "https://uri.etsi.org/ngsi-ld/EntityTypeInfo",
///     crate = "json_ld_engine"
/// )]
/// pub struct EntityTypeInfo {
///     #[jsonld(id)]
///     pub id: String,
///
///     #[jsonld(property = "ngsi:typeName")]
///     pub type_name: String,
/// }
/// ```
pub trait JsonLdExpandable {
    /// Convert this type to its expanded JSON-LD representation.
    ///
    /// Returns a JSON-LD node object with `@id`, `@type` (as an array),
    /// and properties keyed by full IRIs.
    fn to_expanded(&self) -> Value;
}

/// A type that provides dynamic `@type` arrays for JSON-LD expansion.
///
/// Used by types like `Entity` where `@type` is not a compile-time constant
/// but derived from the instance (single or multiple type values).
pub trait JsonLdTypeValue {
    /// Convert this type to a JSON array suitable for the `@type` field.
    fn to_type_array(&self) -> Value;
}

/// A type that can be expanded to a JSON-LD language map.
///
/// Produces an array of `{"@value": "...", "@language": "..."}` objects
/// from a language-indexed map structure (NGSI-LD clause 5.2.32).
pub trait JsonLdLanguageMap {
    /// Convert this language map to its expanded JSON-LD representation.
    fn to_expanded_language_map(&self) -> Value;
}
