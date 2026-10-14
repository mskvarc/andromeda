//! What the whole workspace declares about error carriers and the text they take.
//!
//! One file cannot tell whether the type it constructs is an error, or whether
//! the parameter it hands a string to carries an error's human text: the type
//! and the function are usually declared in another crate. The index reads every
//! production source once, so the walk over each file can ask.

use crate::{
    source_scan::{is_test_only, item_is_test_only},
    string_typed_error::string_shape::type_mentions_string,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    mem,
};
use syn::{
    Attribute,
    Block,
    File,
    FnArg,
    Ident,
    ImplItemFn,
    Item,
    ItemEnum,
    ItemFn,
    ItemImpl,
    ItemStruct,
    Macro,
    Pat,
    Path,
    PathSegment,
    ReturnType,
    Signature,
    Token,
    TraitItemFn,
    Type,
    punctuated::Punctuated,
    visit::{Visit, visit_impl_item_fn, visit_item, visit_item_fn, visit_item_impl, visit_trait_item_fn},
};

/// Parameter and field names that carry the human text of an error or warning.
///
/// A member of this name typed as anything but text cannot receive a `format!`
/// at all, so the name alone marks the sink.
pub(crate) const TEXT_MEMBER_NAMES: &[&str] = &["detail", "error", "message", "reason"];

/// Type-name endings that mark an error or warning carrier even where the type
/// does not implement `Error`: a clause 6.3.17 warning holds its text in an
/// unnamed payload.
const CARRIER_NAME_ENDINGS: &[&str] = &["Error", "Warning"];

/// Macros that build text from a template.
const FORMATTING_MACROS: &[&str] = &["format", "format_args"];

/// The error carriers and text sinks declared across the workspace.
#[derive(Debug, Default)]
pub(crate) struct WorkspaceIndex {
    /// Names of the types that derive or implement `Error`.
    error_types: BTreeSet<String>,
    /// Text-carrying argument positions of path-called functions, keyed by the
    /// bare name of a free function or by `Owner::name` of an associated one.
    path_text_arguments: BTreeMap<String, BTreeSet<usize>>,
    /// Text-carrying argument positions of methods, keyed by method name and
    /// counted after the receiver.
    method_text_arguments: BTreeMap<String, BTreeSet<usize>>,
    /// How the path-called functions of each name build what they return.
    function_texts: BTreeMap<String, ReturnedText>,
    /// How the methods of each name build what they return.
    method_texts: BTreeMap<String, ReturnedText>,
}

/// What the declarations of one function name return, joined over every
/// declaration of that name.
///
/// A name counts as formatting only while every declaration of it formats: a
/// name several types declare (`report`, `key`) says nothing about which one a
/// call reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReturnedText {
    /// Every declaration returns text built with a formatting macro.
    Formatted,
    /// Some declaration returns anything else.
    Other,
}

impl WorkspaceIndex {
    /// The index of every production source.
    pub(crate) fn collect<'a>(sources: impl IntoIterator<Item = &'a File>) -> WorkspaceIndex {
        let mut collector = IndexCollector {
            index: WorkspaceIndex::default(),
            impl_owner: None,
        };
        for syntax in sources {
            collector.visit_file(syntax);
        }
        collector.index
    }

    /// Whether a type name is an error: some production source derives or
    /// implements `Error` for it.
    pub(crate) fn is_error_type(&self, ident: &Ident) -> bool {
        self.error_types.contains(&ident.to_string())
    }

    /// Whether a path segment names an error or warning carrier.
    pub(crate) fn is_carrier(&self, segment: &PathSegment) -> bool {
        let name = segment.ident.to_string();
        self.error_types.contains(&name) || CARRIER_NAME_ENDINGS.iter().any(|ending| name.ends_with(ending))
    }

    /// The text-carrying argument positions of a path call, `owner` being the
    /// type the call is associated with, if any.
    pub(crate) fn path_text_arguments(&self, owner: Option<&str>, function: &Ident) -> BTreeSet<usize> {
        let function = function.to_string();
        let free = self.path_text_arguments.get(&function).into_iter().flatten();
        let associated = owner
            .and_then(|owner| self.path_text_arguments.get(&format!("{owner}::{function}")))
            .into_iter()
            .flatten();
        free.chain(associated).copied().collect()
    }

    /// The text-carrying argument positions of a method call, after the receiver.
    pub(crate) fn method_text_arguments(&self, method: &Ident) -> Option<&BTreeSet<usize>> {
        self.method_text_arguments.get(&method.to_string())
    }

    /// Whether every path-called function of this name returns text built
    /// with a formatting macro.
    pub(crate) fn function_formats_text(&self, function: &Ident) -> bool {
        self.function_texts.get(&function.to_string()) == Some(&ReturnedText::Formatted)
    }

    /// Whether every method of this name returns text built with a formatting
    /// macro.
    pub(crate) fn method_formats_text(&self, method: &Ident) -> bool {
        self.method_texts.get(&method.to_string()) == Some(&ReturnedText::Formatted)
    }

    /// Record the text-carrying parameters of one signature, and the signature
    /// itself when it returns formatted text.
    fn record_signature(&mut self, owner: Option<&Ident>, signature: &Signature, body_formats: bool) {
        let receiver = signature.inputs.iter().any(|input| matches!(input, FnArg::Receiver(_)));
        let typed = signature.inputs.iter().filter(|input| !matches!(input, FnArg::Receiver(_)));
        let positions = typed
            .enumerate()
            .filter(|(_, input)| names_text_member(input))
            .map(|(position, _)| position)
            .collect::<BTreeSet<_>>();
        let name = &signature.ident;
        if !positions.is_empty() {
            if receiver {
                self.method_text_arguments.entry(name.to_string()).or_default().extend(&positions);
            }
            let path_key = owner.map_or_else(|| name.to_string(), |owner| format!("{owner}::{name}"));
            let shift = usize::from(receiver);
            self.path_text_arguments
                .entry(path_key)
                .or_default()
                .extend(positions.iter().map(|position| position + shift));
        }
        let returned = if body_formats && returns_text(&signature.output) {
            ReturnedText::Formatted
        } else {
            ReturnedText::Other
        };
        let texts = if receiver { &mut self.method_texts } else { &mut self.function_texts };
        texts
            .entry(name.to_string())
            .and_modify(|joined| {
                if returned == ReturnedText::Other {
                    *joined = ReturnedText::Other;
                }
            })
            .or_insert(returned);
    }
}

/// Builds the [`WorkspaceIndex`] one source at a time.
struct IndexCollector<'a> {
    /// The index built so far.
    index: WorkspaceIndex,
    /// The type of the `impl` block being read, for associated function keys.
    impl_owner: Option<&'a Ident>,
}

impl<'a> Visit<'a> for IndexCollector<'a> {
    fn visit_item(&mut self, item: &'a Item) {
        if item_is_test_only(item) {
            return;
        }
        visit_item(self, item);
    }

    fn visit_item_struct(&mut self, item: &'a ItemStruct) {
        if derives_error(&item.attrs) {
            self.index.error_types.insert(item.ident.to_string());
        }
    }

    fn visit_item_enum(&mut self, item: &'a ItemEnum) {
        if derives_error(&item.attrs) {
            self.index.error_types.insert(item.ident.to_string());
        }
    }

    fn visit_item_impl(&mut self, item: &'a ItemImpl) {
        let owner = if let Type::Path(self_type) = item.self_ty.as_ref() {
            self_type.path.segments.last().map(|segment| &segment.ident)
        } else {
            None
        };
        let implements_error = item
            .trait_
            .as_ref()
            .is_some_and(|(trait_path, _)| trait_path.segments.last().is_some_and(|segment| segment.ident == "Error"));
        if implements_error {
            self.index.error_types.extend(owner.map(Ident::to_string));
        }
        let enclosing = mem::replace(&mut self.impl_owner, owner);
        visit_item_impl(self, item);
        self.impl_owner = enclosing;
    }

    fn visit_item_fn(&mut self, item: &'a ItemFn) {
        self.index.record_signature(None, &item.sig, block_formats(&item.block));
        visit_item_fn(self, item);
    }

    fn visit_impl_item_fn(&mut self, item: &'a ImplItemFn) {
        if is_test_only(&item.attrs) {
            return;
        }
        self.index.record_signature(self.impl_owner, &item.sig, block_formats(&item.block));
        visit_impl_item_fn(self, item);
    }

    fn visit_trait_item_fn(&mut self, item: &'a TraitItemFn) {
        if is_test_only(&item.attrs) {
            return;
        }
        self.index.record_signature(None, &item.sig, item.default.as_ref().is_some_and(block_formats));
        visit_trait_item_fn(self, item);
    }
}

/// Whether a path names a formatting macro.
pub(crate) fn is_formatting_macro(path: &Path) -> bool {
    path.segments
        .last()
        .is_some_and(|segment| FORMATTING_MACROS.iter().any(|name| segment.ident == name))
}

/// Whether the attributes derive `Error`, under any path spelling
/// (`thiserror::Error`, a `use`-imported `Error`).
fn derives_error(attributes: &[Attribute]) -> bool {
    attributes.iter().filter(|attribute| attribute.path().is_ident("derive")).any(|attribute| {
        attribute
            .parse_args_with(Punctuated::<Path, Token![,]>::parse_terminated)
            .is_ok_and(|derived| derived.iter().any(|path| path.segments.last().is_some_and(|segment| segment.ident == "Error")))
    })
}

/// Whether a parameter is named for the text of an error.
fn names_text_member(input: &FnArg) -> bool {
    let FnArg::Typed(typed) = input else {
        return false;
    };
    let Pat::Ident(pattern) = typed.pat.as_ref() else {
        return false;
    };
    TEXT_MEMBER_NAMES.iter().any(|name| pattern.ident == name)
}

/// Whether a function returns text, directly or inside a standard container.
fn returns_text(output: &ReturnType) -> bool {
    let ReturnType::Type(_, ty) = output else {
        return false;
    };
    type_mentions_string(ty)
}

/// Whether a body invokes a formatting macro anywhere.
fn block_formats(block: &Block) -> bool {
    let mut finder = FormattingMacroFinder { found: false };
    finder.visit_block(block);
    finder.found
}

/// Finds a formatting macro anywhere below the node it visits.
struct FormattingMacroFinder {
    /// Whether one was found.
    found: bool,
}

impl<'a> Visit<'a> for FormattingMacroFinder {
    fn visit_macro(&mut self, mac: &'a Macro) {
        self.found |= is_formatting_macro(&mac.path);
    }
}
