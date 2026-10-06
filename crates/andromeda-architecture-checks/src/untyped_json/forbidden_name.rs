//! Which paths name an untyped JSON tree, and where the processor's own tree
//! may still be named.

/// The JSON processor's working tree is admitted only below these sources: the
/// engine that hands documents to the JSON-LD processor, and the crate that
/// holds a JSON literal as text and parses it transiently for the few tree
/// operations a literal needs.
const PROCESSOR_TREE_OWNERS: [&str; 2] = ["crates/andromeda-jsonld/src/engine/", "crates/andromeda-json-literal/"];

/// The members of `serde_json` (and of its `value` module) that build, hold or
/// convert an untyped tree.
const SERDE_JSON_TREE_MEMBERS: [&str; 5] = ["Value", "Map", "json", "to_value", "from_value"];

/// The tree a forbidden name belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UntypedTree {
    /// `serde_json::Value` and the functions and macro that build or convert it.
    SerdeJson,
    /// The JSON processor's `jstrict` tree, re-exported as `jsonld_syntax::Value`
    /// and again as `jsonld::syntax::Value`.
    Processor,
}

impl UntypedTree {
    /// Whether a source at `path` may name this tree.
    pub(crate) fn admitted_in(self, path: &str) -> bool {
        match self {
            UntypedTree::SerdeJson => false,
            UntypedTree::Processor => PROCESSOR_TREE_OWNERS.iter().any(|owner| path.starts_with(owner)),
        }
    }
}

/// The tree a fully qualified path names, if it names one.
///
/// `segments` is the whole path as written, so `serde_json::Value::Object`
/// matches through its `Value` segment and `serde_json::value::RawValue` (text,
/// not a tree) does not match.
pub(crate) fn untyped_tree_of(segments: &[String]) -> Option<UntypedTree> {
    let segments: Vec<&str> = segments.iter().map(String::as_str).collect();
    match segments.as_slice() {
        ["serde_json", member, ..] | ["serde_json", "value", member, ..] if SERDE_JSON_TREE_MEMBERS.contains(member) => Some(UntypedTree::SerdeJson),
        ["jstrict", ..] | ["jsonld_syntax", "Value", ..] | ["jsonld", "syntax", "Value", ..] => Some(UntypedTree::Processor),
        _ => None,
    }
}

/// Whether a path's last segment is the `json!` macro, which a source names
/// only as a macro call.
pub(crate) fn names_the_json_macro(segments: &[String]) -> bool {
    segments.last().is_some_and(|last| last == "json")
}
