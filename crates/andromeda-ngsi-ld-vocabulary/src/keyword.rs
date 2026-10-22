//! The JSON-LD keywords an NGSI-LD document carries (JSON-LD 1.1 section 1.7).
//!
//! A keyword is no term of any `@context`, so the generated vocabulary holds
//! none of them, nor the two terms the core context aliases to keywords. Every
//! reader and writer that names one outside a serde attribute, which takes a
//! literal, names it here.

/// `@context`: the context a document is interpreted against.
pub const CONTEXT: &str = "@context";

/// `@id`: a node's identifier.
pub const ID: &str = "@id";

/// `id`, the term the core context aliases to `@id`: the member an Entity
/// names its identifier in (clause 4.5.1).
pub const ID_ALIAS: &str = "id";

/// `@import`: a context that a scoped context imports (JSON-LD 1.1 section
/// 4.1.10).
pub const IMPORT: &str = "@import";

/// `@json`: the datatype of a JSON literal (JSON-LD 1.1 section 4.2.2).
pub const JSON: &str = "@json";

/// `@language`: the language of a string value.
pub const LANGUAGE: &str = "@language";

/// `@list`: an ordered list.
pub const LIST: &str = "@list";

/// `@none`: the index of a value no index applies to (JSON-LD 1.1 section
/// 4.6). NGSI-LD gives it several readings: the default attribute instance of
/// clause 4.5.5, the local broker of an `EntityMap` (clause 4.5.25), the
/// untagged string of a `languageMap` (clause 4.5.18), the Linked Entity
/// retrieval that joins nothing (clause 4.5.23) and the key a grouped temporal
/// representation places an absent value under.
pub const NONE: &str = "@none";

/// `@type`: a node's types or a value's datatype.
pub const TYPE: &str = "@type";

/// `type`, the term the core context aliases to `@type`: the member an
/// Entity names its types in, an attribute instance its family and a `GeoJSON`
/// geometry its geometry type (clauses 4.5.1, 4.5.2.2 and 4.7).
pub const TYPE_ALIAS: &str = "type";

/// `@value`: the content of a value object.
pub const VALUE: &str = "@value";

#[cfg(test)]
mod tests {
    use crate::keyword::{CONTEXT, ID, ID_ALIAS, IMPORT, JSON, LANGUAGE, LIST, NONE, TYPE, TYPE_ALIAS, VALUE};

    /// JSON-LD 1.1 section 1.7 spells every keyword with a leading `@`.
    #[test]
    fn every_keyword_is_spelled_as_json_ld_1_1_defines_it() {
        pretty_assertions::assert_eq!(
            [CONTEXT, ID, IMPORT, JSON, LANGUAGE, LIST, NONE, TYPE, VALUE],
            ["@context", "@id", "@import", "@json", "@language", "@list", "@none", "@type", "@value"]
        );
    }

    /// Annex B: the core context maps `id` to `@id` and `type` to `@type`.
    #[test]
    fn the_core_context_aliases_name_their_keywords_without_the_at_sign() {
        pretty_assertions::assert_eq!([ID_ALIAS, TYPE_ALIAS], [&ID[1..], &TYPE[1..]]);
    }
}
