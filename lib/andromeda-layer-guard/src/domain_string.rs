//! Exact-declaration inventory for production domain-string types.

use crate::source_scan::{is_test_only, workspace_sources};
use quote::ToTokens;
use std::{collections::BTreeSet, path::Path};
use syn::{
    Block,
    Fields,
    FnArg,
    GenericArgument,
    ImplItem,
    Item,
    PatType,
    PathArguments,
    ReturnType,
    TraitItem,
    Type,
    TypeParamBound,
    punctuated::Punctuated,
    token::Comma,
    visit::{Visit, visit_pat_type, visit_type, visit_type_path},
};

/// A reason a string-shaped production declaration is legitimate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StringRationale {
    /// Unstructured text written or read by a person.
    FreeFormText,
    /// A JSON object member name whose vocabulary is not fixed.
    JsonMemberName,
    /// SQL text or an already-rendered wire fragment.
    SqlOrWireText,
    /// Opaque payload text owned by an external format.
    OpaquePayload,
    /// Storage inside a validated newtype that exposes no unchecked constructor.
    ValidatedNewtypeStorage,
}

/// Assert that actual declarations exactly equal a tab-separated inventory.
///
/// Each non-comment line contains a [`StringRationale`] variant name, a tab,
/// and one exact declaration. Keeping the large workspace audit in a data file
/// makes every exception reviewable without weakening exact matching.
///
/// # Panics
///
/// Panics when a line is malformed, a rationale is unknown, a declaration is
/// duplicated, or the inventory differs from production source.
pub fn assert_domain_string_inventory_text(workspace_root: &str, inventory: &str) {
    let mut allowed = BTreeSet::new();
    for (index, line) in inventory.lines().enumerate() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (rationale, declaration) = line
            .split_once('\t')
            .unwrap_or_else(|| unreachable!("domain-string inventory line {} has no tab", index + 1));
        match rationale {
            "FreeFormText" | "JsonMemberName" | "SqlOrWireText" | "OpaquePayload" | "ValidatedNewtypeStorage" => {}
            other => unreachable!("domain-string inventory line {} has unknown rationale {other}", index + 1),
        }
        assert!(
            allowed.insert(declaration.to_owned()),
            "duplicate domain-string inventory declaration: {declaration}"
        );
    }

    let actual = domain_string_declarations(workspace_root).into_iter().collect::<BTreeSet<_>>();
    let unexpected = actual.difference(&allowed).cloned().collect::<Vec<_>>();
    let stale = allowed.difference(&actual).cloned().collect::<Vec<_>>();
    assert!(
        unexpected.is_empty() && stale.is_empty(),
        "domain-string inventory mismatch\nunexpected:\n{}\nstale:\n{}",
        unexpected.join("\n"),
        stale.join("\n")
    );
}

/// Return all production declarations containing a forbidden string-shaped type.
pub fn domain_string_declarations(workspace_root: &str) -> Vec<String> {
    let mut declarations = Vec::new();
    for source in workspace_sources(Path::new(workspace_root)) {
        let mut collector = DeclarationCollector::new(&source.workspace_path);
        collector.collect_items(&source.syntax.items);
        declarations.extend(collector.declarations);
    }
    declarations.sort();
    declarations.dedup();
    declarations
}

/// Collect forbidden types from declaration surfaces, including typed locals.
struct DeclarationCollector<'path> {
    /// Exact declarations found so far.
    declarations: Vec<String>,
    /// Nested inline-module path.
    modules: Vec<String>,
    /// Workspace-relative source path.
    path: &'path str,
}

impl<'path> DeclarationCollector<'path> {
    /// Create a collector for one source file.
    const fn new(path: &'path str) -> DeclarationCollector<'path> {
        DeclarationCollector {
            declarations: Vec::new(),
            modules: Vec::new(),
            path,
        }
    }

    /// Collect every relevant item in one module.
    fn collect_items(&mut self, items: &[Item]) {
        items.iter().for_each(|item| self.collect_item(item));
    }

    /// Collect one item and recurse only into production inline modules.
    fn collect_item(&mut self, item: &Item) {
        match item {
            Item::Const(item) if !is_test_only(&item.attrs) => self.record(&item.ident.to_string(), &item.ty),
            Item::Enum(item) if !is_test_only(&item.attrs) => item.variants.iter().for_each(|variant| {
                self.collect_fields(&format!("{}::{}", item.ident, variant.ident), &variant.fields);
            }),
            Item::Fn(item) if !is_test_only(&item.attrs) => {
                let function = item.sig.ident.to_string();
                self.collect_signature(&function, &item.sig.inputs, &item.sig.output);
                self.collect_locals(&function, &item.block);
            }
            Item::Impl(item) if !is_test_only(&item.attrs) => {
                let owner = item.self_ty.to_token_stream().to_string();
                item.items.iter().for_each(|member| match member {
                    ImplItem::Const(member) if !is_test_only(&member.attrs) => {
                        self.record(&format!("{owner}::{}", member.ident), &member.ty);
                    }
                    ImplItem::Fn(member) if !is_test_only(&member.attrs) => {
                        let function = format!("{owner}::{}", member.sig.ident);
                        self.collect_signature(&function, &member.sig.inputs, &member.sig.output);
                        self.collect_locals(&function, &member.block);
                    }
                    ImplItem::Type(member) if !is_test_only(&member.attrs) => {
                        self.record(&format!("{owner}::{}", member.ident), &member.ty);
                    }
                    ImplItem::Macro(_) | ImplItem::Verbatim(_) | ImplItem::Const(_) | ImplItem::Fn(_) | ImplItem::Type(_) => {}
                    _ => {}
                });
            }
            Item::Mod(item) if !is_test_only(&item.attrs) => {
                if let Some((_, items)) = &item.content {
                    self.modules.push(item.ident.to_string());
                    self.collect_items(items);
                    self.modules.pop();
                }
            }
            Item::Static(item) if !is_test_only(&item.attrs) => self.record(&item.ident.to_string(), &item.ty),
            Item::Struct(item) if !is_test_only(&item.attrs) => self.collect_fields(&item.ident.to_string(), &item.fields),
            Item::Trait(item) if !is_test_only(&item.attrs) => item.items.iter().for_each(|member| match member {
                TraitItem::Const(member) if !is_test_only(&member.attrs) => {
                    self.record(&format!("{}::{}", item.ident, member.ident), &member.ty);
                }
                TraitItem::Fn(member) if !is_test_only(&member.attrs) => {
                    let function = format!("{}::{}", item.ident, member.sig.ident);
                    self.collect_signature(&function, &member.sig.inputs, &member.sig.output);
                    if let Some(block) = &member.default {
                        self.collect_locals(&function, block);
                    }
                }
                TraitItem::Type(member) if !is_test_only(&member.attrs) => {
                    member.bounds.iter().for_each(|bound| {
                        if let TypeParamBound::Trait(bound) = bound {
                            bound.path.segments.iter().for_each(|segment| {
                                if segment.ident == "String" || segment.ident == "CompactString" {
                                    self.record_text(&format!("{}::{}", item.ident, member.ident), &bound.to_token_stream().to_string());
                                }
                            });
                        }
                    });
                }
                TraitItem::Macro(_) | TraitItem::Verbatim(_) | TraitItem::Const(_) | TraitItem::Fn(_) | TraitItem::Type(_) => {}
                _ => {}
            }),
            Item::Type(item) if !is_test_only(&item.attrs) => self.record(&item.ident.to_string(), &item.ty),
            Item::Union(item) if !is_test_only(&item.attrs) => {
                self.collect_fields(&item.ident.to_string(), &Fields::Named(item.fields.clone()));
            }
            Item::Const(_)
            | Item::Enum(_)
            | Item::ExternCrate(_)
            | Item::Fn(_)
            | Item::ForeignMod(_)
            | Item::Impl(_)
            | Item::Macro(_)
            | Item::Mod(_)
            | Item::Static(_)
            | Item::Struct(_)
            | Item::Trait(_)
            | Item::TraitAlias(_)
            | Item::Type(_)
            | Item::Union(_)
            | Item::Use(_)
            | Item::Verbatim(_) => {}
            _ => {}
        }
    }

    /// Collect every named or positional field.
    fn collect_fields(&mut self, owner: &str, fields: &Fields) {
        fields.iter().enumerate().for_each(|(index, field)| {
            if !is_test_only(&field.attrs) {
                let member = field.ident.as_ref().map_or_else(|| index.to_string(), ToString::to_string);
                self.record(&format!("{owner}::{member}"), &field.ty);
            }
        });
    }

    /// Collect function parameters and return type.
    fn collect_signature(&mut self, function: &str, inputs: &Punctuated<FnArg, Comma>, output: &ReturnType) {
        inputs.iter().for_each(|input| {
            if let FnArg::Typed(argument) = input {
                self.record(&format!("{function}::parameter {}", argument.pat.to_token_stream()), &argument.ty);
            }
        });
        if let ReturnType::Type(_, ty) = output {
            self.record(&format!("{function}::return"), ty);
        }
    }

    /// Collect explicitly typed local and closure declarations in a function body.
    fn collect_locals(&mut self, function: &str, block: &Block) {
        let mut visitor = TypedLocalVisitor { declarations: Vec::new() };
        visitor.visit_block(block);
        visitor.declarations.into_iter().for_each(|(pattern, ty)| {
            self.record_text(&format!("{function}::local {pattern}"), &ty);
        });
    }

    /// Record a type if it contains one of the audited forms.
    fn record(&mut self, symbol: &str, ty: &Type) {
        let mut visitor = ForbiddenTypeVisitor::default();
        visitor.visit_type(ty);
        if visitor.found {
            self.record_text(symbol, &ty.to_token_stream().to_string());
        }
    }

    /// Record a normalized exact declaration.
    fn record_text(&mut self, symbol: &str, ty: &str) {
        let module = if self.modules.is_empty() {
            String::new()
        } else {
            format!("{}::", self.modules.join("::"))
        };
        self.declarations.push(format!("{}::{module}{symbol}: {ty}", self.path));
    }
}

/// Collect explicitly typed patterns from one production function body.
struct TypedLocalVisitor {
    /// Pattern and normalized type pairs found in the body.
    declarations: Vec<(String, String)>,
}

impl<'ast> Visit<'ast> for TypedLocalVisitor {
    fn visit_pat_type(&mut self, pattern: &'ast PatType) {
        let mut visitor = ForbiddenTypeVisitor::default();
        visitor.visit_type(&pattern.ty);
        if visitor.found {
            self.declarations
                .push((pattern.pat.to_token_stream().to_string(), pattern.ty.to_token_stream().to_string()));
        }
        visit_pat_type(self, pattern);
    }
}

/// Detect any banned string-shaped type nested inside a declaration type.
#[derive(Default)]
struct ForbiddenTypeVisitor {
    /// Whether a forbidden type has been observed.
    found: bool,
}

impl<'ast> Visit<'ast> for ForbiddenTypeVisitor {
    fn visit_type(&mut self, ty: &'ast Type) {
        if matches!(ty, Type::Reference(reference) if matches!(reference.elem.as_ref(), Type::Path(path) if path.path.is_ident("str"))) {
            self.found = true;
        }
        visit_type(self, ty);
    }

    fn visit_type_path(&mut self, ty: &'ast syn::TypePath) {
        if ty.path.segments.last().is_some_and(|segment| {
            segment.ident == "String"
                || segment.ident == "CompactString"
                || ((segment.ident == "Cow" || segment.ident == "Box")
                    && matches!(
                        &segment.arguments,
                        PathArguments::AngleBracketed(arguments)
                            if arguments.args.iter().any(|argument| matches!(
                                argument,
                                GenericArgument::Type(Type::Path(path)) if path.path.is_ident("str")
                            ))
                    ))
        }) {
            self.found = true;
        }
        visit_type_path(self, ty);
    }
}

#[cfg(test)]
mod tests {
    use crate::domain_string::DeclarationCollector;
    use std::collections::BTreeSet;

    /// Collect declarations from one source fixture.
    fn fixture(source: &str) -> Vec<String> {
        let syntax = syn::parse_file(source).unwrap_or_else(|error| unreachable!("fixture parses: {error}"));
        let mut collector = DeclarationCollector::new("fixture.rs");
        collector.collect_items(&syntax.items);
        collector.declarations.sort();
        collector.declarations
    }

    #[test]
    fn finds_every_forbidden_shape_in_production_declarations() {
        let declarations = fixture(
            "struct Shapes<'a> { owned: String, borrowed: &'a str, cow: std::borrow::Cow<'a, str>, boxed: Box<str>, compact: CompactString, map: HashMap<String, String> }",
        );
        assert_eq!(declarations.len(), 6);
    }

    #[test]
    fn finds_explicitly_typed_locals_and_closure_parameters() {
        let declarations = fixture("fn declarations() { let map: HashMap<String, Value> = HashMap::new(); let render = |wire: &str| wire.len(); }");
        assert_eq!(declarations.len(), 2);
    }

    #[test]
    fn ignores_test_only_declarations() {
        let declarations = fixture("#[cfg(test)] mod tests { struct Hidden { value: String } }");
        assert!(declarations.is_empty());
    }

    #[test]
    fn exact_inventory_detects_replacement_at_an_unchanged_count() {
        let allowed = fixture("struct First { text: String }").into_iter().collect::<BTreeSet<_>>();
        let actual = fixture("struct Second { text: String }").into_iter().collect::<BTreeSet<_>>();
        assert_eq!(allowed.len(), actual.len());
        assert_eq!(actual.difference(&allowed).count(), 1);
        assert_eq!(allowed.difference(&actual).count(), 1);
    }

    #[test]
    fn legitimate_shapes_still_require_exact_documented_exemptions() {
        let declarations =
            fixture("struct Legitimate<'a> { description: String, json: HashMap<String, Value>, sql: &'a str, wire: Cow<'a, str>, scope_storage: String }");
        assert_eq!(declarations.len(), 5);
    }
}
