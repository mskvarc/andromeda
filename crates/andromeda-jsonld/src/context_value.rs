//! The contexts one `@context` value names, as written.
//!
//! JSON-LD 1.1 section 9.15 gives a `@context` value exactly four shapes: `null`,
//! a reference to a remote context, a context definition written in place, or
//! an array of those. Everything the broker decides about a context a request
//! named is decided entry by entry: which core references it drops (clause
//! 5.5.7), what it leaves in force last (clause 4.4), and which documents it
//! reaches. [`ContextEntry`] is that reading of an admitted `@context` text,
//! each entry still a slice of it.
//!
//! The value a document is processed under, and the one a response names, are
//! assembled from such entries ([`written_entries`]): every entry is already
//! JSON text, so the assembly writes text and builds no tree.

use crate::core_context_precedence::{defines_nothing_but_an_import, is_core_context_json_string};
use andromeda_json_literal::{
    json_text::quoted,
    piece::{JsonKind, JsonPiece},
};
use andromeda_ngsi_ld_vocabulary::keyword;
use std::borrow::Cow;

/// One context a `@context` value names.
#[derive(Debug, Clone)]
pub enum ContextEntry<'a> {
    /// `null`, which clears the active context (JSON-LD 1.1 section 4.1).
    Null,
    /// A reference to a remote context document, as written.
    Reference(Cow<'a, str>),
    /// A context definition written in place.
    Definition(ContextDefinition<'a>),
    /// A value no `@context` entry takes, such as a boolean, a number or a
    /// nested array, kept as written for the processor to refuse.
    Malformed(JsonPiece<'a>),
}

/// A context definition written in place, possibly without the `@import` of
/// a core context the broker dropped from it.
#[derive(Debug, Clone, Copy)]
pub struct ContextDefinition<'a> {
    /// The definition object as written.
    written: JsonPiece<'a>,
    /// Whether its `@import` of a core context was dropped.
    core_import: CoreImport,
}

/// Whether a definition still names the core context it imported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreImport {
    /// The definition imports no core context, or still imports the one it
    /// names.
    Kept,
    /// Its `@import` of a core context is left out.
    Dropped,
}

/// The entries of a `@context` value, in the order JSON-LD processes them:
/// none for `null`, each element of an array, and the value itself otherwise.
#[must_use]
pub fn context_entries(context: JsonPiece<'_>) -> Vec<ContextEntry<'_>> {
    match context.kind() {
        JsonKind::Null => Vec::new(),
        JsonKind::Array => context.elements().unwrap_or_default().into_iter().map(ContextEntry::of).collect(),
        JsonKind::Boolean | JsonKind::Number | JsonKind::String | JsonKind::Object => vec![ContextEntry::of(context)],
    }
}

impl<'a> ContextEntry<'a> {
    /// One element of a `@context` value, read as the entry it is.
    #[must_use]
    pub fn of(entry: JsonPiece<'a>) -> ContextEntry<'a> {
        match entry.kind() {
            JsonKind::Null => ContextEntry::Null,
            JsonKind::String => entry.as_str().map_or(ContextEntry::Malformed(entry), ContextEntry::Reference),
            JsonKind::Object => ContextEntry::Definition(ContextDefinition {
                written: entry,
                core_import: CoreImport::Kept,
            }),
            JsonKind::Boolean | JsonKind::Number | JsonKind::Array => ContextEntry::Malformed(entry),
        }
    }

    /// A reference to `url`.
    #[must_use]
    pub const fn reference(url: &'a str) -> ContextEntry<'a> {
        ContextEntry::Reference(Cow::Borrowed(url))
    }

    /// Write the entry as JSON text onto `written`.
    pub(crate) fn write(&self, written: &mut String) {
        match self {
            ContextEntry::Null => written.push_str("null"),
            ContextEntry::Reference(reference) => written.push_str(&quoted(reference)),
            ContextEntry::Definition(definition) => definition.write(written),
            ContextEntry::Malformed(piece) => written.push_str(piece.text().trim()),
        }
    }
}

impl<'a> ContextDefinition<'a> {
    /// The definition object as written.
    #[must_use]
    pub const fn written(&self) -> JsonPiece<'a> {
        self.written
    }

    /// The context the definition imports (JSON-LD 1.1 section 4.1.10), unless
    /// the broker dropped that import.
    #[must_use]
    pub fn import(&self) -> Option<Cow<'a, str>> {
        match self.core_import {
            CoreImport::Kept => self.written.member(keyword::IMPORT).and_then(JsonPiece::as_str),
            CoreImport::Dropped => None,
        }
    }

    /// The definition with its `@import` of a core context dropped, and
    /// whether anything of its own is left.
    ///
    /// JSON-LD 1.1 section 4.1.10 merges an imported context into the
    /// definition that imports it, so an imported core context is an earlier,
    /// protected occurrence of it; clause 5.5.7 gives the core context
    /// absolute precedence however a user context names it, so the import goes
    /// like every other core reference. A definition that set nothing but the
    /// import, `@version` and `@protected` defined nothing of its own.
    fn without_core_import(self) -> DefinitionWithoutCore<'a> {
        let imports_core = self
            .written
            .member(keyword::IMPORT)
            .and_then(JsonPiece::as_str)
            .is_some_and(|import| is_core_context_json_string(&import));
        if !imports_core {
            return DefinitionWithoutCore::Unchanged(self);
        }
        let members = self.written.members().unwrap_or_default();
        if defines_nothing_but_an_import(members.iter().map(|(key, _)| key.as_ref())) {
            DefinitionWithoutCore::Emptied
        } else {
            DefinitionWithoutCore::ImportDropped(ContextDefinition {
                written: self.written,
                core_import: CoreImport::Dropped,
            })
        }
    }

    /// Write the definition as JSON text onto `written`, leaving out a dropped
    /// `@import`.
    fn write(&self, written: &mut String) {
        match self.core_import {
            CoreImport::Kept => written.push_str(self.written.text().trim()),
            CoreImport::Dropped => {
                written.push('{');
                let kept = self.written.members().unwrap_or_default().into_iter().filter(|(key, _)| key != keyword::IMPORT);
                for (index, (key, value)) in kept.enumerate() {
                    if index > 0 {
                        written.push(',');
                    }
                    written.push_str(&quoted(&key));
                    written.push(':');
                    written.push_str(value.text().trim());
                }
                written.push('}');
            }
        }
    }
}

/// A definition once its `@import` of a core context is dropped.
enum DefinitionWithoutCore<'a> {
    /// It imported no core context.
    Unchanged(ContextDefinition<'a>),
    /// It imported one and defines something of its own besides.
    ImportDropped(ContextDefinition<'a>),
    /// It imported one and defined nothing else.
    Emptied,
}

/// The contexts a request named once every core reference among them is taken
/// out, directly or through an `@import`.
#[derive(Debug, Clone)]
pub struct WithoutCore<'a> {
    /// The remaining contexts in order, repeats included.
    pub kept: Vec<ContextEntry<'a>>,
    /// Whether a core reference was taken out.
    pub named_core: NamedCore,
}

/// Whether the contexts a request named included a core context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamedCore {
    /// They named one, directly or through an `@import`.
    Yes,
    /// They named none.
    No,
}

/// Take every core reference out of `entries`.
///
/// Repeats are kept: JSON-LD processes an `@context` array in order, so
/// `[A, B, A]` leaves A's definitions in force where `[A, B]` leaves B's, and
/// clause 5.5.7 processes the context the call provided. A definition that held
/// nothing but the core import (and `@version` or `@protected`) is removed with
/// it.
#[must_use]
pub fn without_core(entries: Vec<ContextEntry<'_>>) -> WithoutCore<'_> {
    let mut named_core = NamedCore::No;
    let mut kept = Vec::with_capacity(entries.len() + 1);
    for entry in entries {
        match entry {
            ContextEntry::Reference(reference) if is_core_context_json_string(&reference) => named_core = NamedCore::Yes,
            ContextEntry::Definition(definition) => match definition.without_core_import() {
                DefinitionWithoutCore::Unchanged(definition) => kept.push(ContextEntry::Definition(definition)),
                DefinitionWithoutCore::ImportDropped(definition) => {
                    named_core = NamedCore::Yes;
                    kept.push(ContextEntry::Definition(definition));
                }
                DefinitionWithoutCore::Emptied => named_core = NamedCore::Yes,
            },
            other @ (ContextEntry::Null | ContextEntry::Reference(_) | ContextEntry::Malformed(_)) => kept.push(other),
        }
    }
    WithoutCore { kept, named_core }
}

/// How several entries are written as one `@context` value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntriesShape {
    /// Always as an array.
    Array,
    /// A single entry on its own, several as an array (clause 5.2.3: "flattened
    /// to a single string if necessary").
    Flattened,
}

/// `entries` written as one `@context` value.
#[must_use]
pub fn written_entries(entries: &[ContextEntry<'_>], shape: EntriesShape) -> String {
    let mut written = String::new();
    match (shape, entries) {
        (EntriesShape::Flattened, [single]) => single.write(&mut written),
        (EntriesShape::Array | EntriesShape::Flattened, several) => {
            written.push('[');
            for (index, entry) in several.iter().enumerate() {
                if index > 0 {
                    written.push(',');
                }
                entry.write(&mut written);
            }
            written.push(']');
        }
    }
    written
}

#[cfg(test)]
mod tests {
    use crate::context_value::{ContextEntry, EntriesShape, NamedCore, context_entries, without_core, written_entries};
    use andromeda_json_literal::JsonLiteral;
    use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
    use serde_json::json;

    fn literal(value: &serde_json::Value) -> JsonLiteral {
        JsonLiteral::admit(&value.to_string()).expect("an admissible context")
    }

    #[test]
    fn an_array_names_its_elements_and_null_names_nothing() {
        let context = literal(&json!(["https://example.com/a.jsonld", {"t": "https://example.com/t"}, null, 3]));
        let entries = context_entries(context.piece());
        assert!(matches!(
            entries.as_slice(),
            [
                ContextEntry::Reference(_),
                ContextEntry::Definition(_),
                ContextEntry::Null,
                ContextEntry::Malformed(_)
            ]
        ));

        let null = JsonLiteral::null();
        assert!(context_entries(null.piece()).is_empty());
    }

    /// Clause 5.5.7: every core reference is taken out, a definition importing
    /// the core context loses the import and is removed when nothing else is
    /// left of it.
    #[test]
    fn core_references_and_core_imports_are_taken_out() {
        let core = NGSI_LD_CORE_CONTEXT_URL.as_str();
        let context = literal(&json!([
            core,
            "https://example.com/a.jsonld",
            {"@import": core, "@version": 1.1},
            {"@import": core, "t": "https://example.com/t"},
            "https://example.com/a.jsonld"
        ]));

        let retained = without_core(context_entries(context.piece()));

        pretty_assertions::assert_eq!(retained.named_core, NamedCore::Yes);
        pretty_assertions::assert_eq!(
            written_entries(&retained.kept, EntriesShape::Array),
            r#"["https://example.com/a.jsonld",{"t":"https://example.com/t"},"https://example.com/a.jsonld"]"#
        );
    }

    #[test]
    fn a_context_naming_no_core_keeps_everything() {
        let context = literal(&json!({"@import": "https://example.com/base.jsonld", "t": "https://example.com/t"}));
        let retained = without_core(context_entries(context.piece()));
        pretty_assertions::assert_eq!(retained.named_core, NamedCore::No);
        pretty_assertions::assert_eq!(
            written_entries(&retained.kept, EntriesShape::Flattened),
            r#"{"@import":"https://example.com/base.jsonld","t":"https://example.com/t"}"#
        );
    }

    #[test]
    fn a_flattened_single_reference_is_the_string_and_several_are_an_array() {
        let one = [ContextEntry::reference("https://example.com/a.jsonld")];
        pretty_assertions::assert_eq!(written_entries(&one, EntriesShape::Flattened), r#""https://example.com/a.jsonld""#);
        pretty_assertions::assert_eq!(written_entries(&one, EntriesShape::Array), r#"["https://example.com/a.jsonld"]"#);
        pretty_assertions::assert_eq!(written_entries(&[], EntriesShape::Flattened), "[]");
    }
}
