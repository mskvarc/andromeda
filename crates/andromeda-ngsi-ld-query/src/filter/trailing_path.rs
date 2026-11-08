//! The trailing `[…]` group of a clause 4.9 attribute path.
//!
//! The clause 4.9 ABNF writes the group as `[DottedPath]`, a member expression
//! into a compound value, and its prose gives a `LanguageProperty`'s group a
//! second reading, the target language (`color[en]`). Annex C.5.13.2 writes a
//! target language no `AttrName` can spell (`marque[en-GB]`), and `[*]` selects
//! every language. The three shapes are the variants of [`TrailingPath`].

use crate::filter::target_language::TargetLanguage;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;

/// What a path's trailing `[…]` group holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrailingPath {
    /// A dot-separated member path (`address[city]`,
    /// `rawdata[airquality.particulate]`). A one-member path whose member is
    /// also a language tag (`color[en]`) is a target language when the
    /// attribute is a `LanguageProperty`.
    Members(MemberPath),
    /// A target language only a language tag or range can spell
    /// (`marque[en-GB]`, `name[de-*-DE]`).
    Language(TargetLanguage),
    /// The `[*]` marker, which matches a `LanguageProperty` against every
    /// language's value (clause 4.9).
    AllLanguages,
}

/// The members of a compound-value path, in the two spellings the evaluator
/// reads them by.
#[derive(Debug, Clone)]
pub struct MemberPath {
    /// The members as the path addresses them.
    ///
    /// Typed as [`AttributeName`], not as free-form JSON keys, because that is
    /// what they address. JSON-LD expansion turns the members of a structured
    /// Property value into IRI-keyed nodes exactly as it turns attribute names
    /// into IRIs, so a member name arrives compact and must be expanded through
    /// the request `@context` before it can address anything the broker stored.
    pub addressed: Vec<AttributeName>,
    /// The members as the client wrote them, same length and order.
    ///
    /// A `JsonProperty`'s (or an opaque Property's) raw JSON object keeps the
    /// keys its writer chose, which expansion never touches, and a target
    /// language is never a term a `@context` expands. Equal to `addressed`
    /// until expansion rewrites `addressed` alone.
    pub written: Vec<AttributeName>,
}

// Hand-written because the field to compare is chosen, not both of them:
// `written` is a retry spelling carried alongside `addressed`, not part of
// what a path addresses, so two paths reaching the same location are equal
// whichever spelling they carry. The sanctioned derive for a skipped field is
// `educe`, and a proc-macro dependency in this crate is not worth one skipped
// field.
impl PartialEq for MemberPath {
    fn eq(&self, other: &MemberPath) -> bool {
        self.addressed == other.addressed
    }
}

impl Eq for MemberPath {}

impl MemberPath {
    /// A member path as freshly parsed, its two spellings the same.
    #[must_use]
    pub fn new(members: Vec<AttributeName>) -> MemberPath {
        // clone: the two spellings are separate values, since expansion rewrites only the addressed one.
        let written = members.clone();
        MemberPath { addressed: members, written }
    }
}
