//! The syntax walk that finds, in one parsed source, each mark a second copy
//! of Linked Entity retrieval leaves.

use crate::{linked_retrieval::rule::Rule, source_scan::item_is_test_only};
use syn::{
    Expr,
    ExprMethodCall,
    ExprPath,
    ExprStruct,
    ImplItemFn,
    Item,
    ItemEnum,
    ItemFn,
    ItemImpl,
    ItemStruct,
    ItemTrait,
    ItemType,
    Macro,
    Pat,
    PathArguments,
    TraitItemFn,
    Type,
    visit::{
        Visit,
        visit_expr_method_call,
        visit_expr_path,
        visit_expr_struct,
        visit_impl_item_fn,
        visit_item,
        visit_item_enum,
        visit_item_fn,
        visit_item_impl,
        visit_item_struct,
        visit_item_trait,
        visit_item_type,
        visit_macro,
        visit_pat,
        visit_path,
        visit_trait_item_fn,
    },
};

/// The calls that read a nested projection term.
const NESTED_TERM_READS: [&str; 3] = ["linked_terms", "linked_depth", "matching_term"];

/// The types whose `Flat` and `Inline` variants are join modes.
const JOIN_MODE_TYPES: [&str; 2] = ["JoinMode", "RetrievingJoin"];

/// The retrieving join modes.
const RETRIEVING_MODES: [&str; 2] = ["Flat", "Inline"];

/// The names the inline members are imported under.
const INLINE_MEMBER_MODULES: [&str; 2] = ["compact_properties", "properties"];

/// The clause 4.5.23.2 inline members.
const INLINE_MEMBERS: [&str; 2] = ["ENTITY", "ENTITY_LIST"];

/// The calls whose first argument is the key of a member they write.
const MEMBER_WRITES: [&str; 4] = ["insert", "entry", "serialize_entry", "serialize_field"];

/// The `ParameterError` variants only `From<AdmissionError>` may build.
const ADMISSION_VARIANTS: [&str; 6] = [
    "ProjectionLinkedTermsWithoutJoin",
    "ProjectionDepthExceedsJoinLevel",
    "FilterTraversalWithoutJoin",
    "FilterDepthExceedsJoinLevel",
    "LinkedRetrievalUnavailable",
    "JoinLevelWithoutJoin",
];

/// The engine's adapter traits.
const ADAPTER_TRAITS: [&str; 2] = ["LinkedSource", "LinkedRepresentation"];

/// The names every private walk so far has used.
const WALK_NAMES: [&str; 5] = ["JoinPath", "Ancestor", "LinkedCandidate", "resolve_linked", "traverse"];

/// Collects what one parsed source decides about Linked Entity retrieval.
#[derive(Default)]
pub(crate) struct LinkedRetrievalVisitor {
    /// How many patterns enclose the current node.
    pattern_depth: usize,
    /// How many `From<AdmissionError> for ParameterError` impls enclose the
    /// current node.
    admission_conversion_depth: usize,
    /// Whether the source builds an `AttributeIriIndex` through `new`.
    builds_index: bool,
    /// The `insert` calls, which build an index only in a source building one.
    inserts: Vec<String>,
    /// The findings that hold whatever the source names.
    findings: Vec<(Rule, String)>,
}

impl LinkedRetrievalVisitor {
    /// Every finding this source commits, each with the rule it breaks.
    pub(crate) fn into_findings(self) -> Vec<(Rule, String)> {
        let mut findings = self.findings;
        if self.builds_index {
            findings.extend(self.inserts.into_iter().map(|insert| (Rule::IndexBuild, insert)));
        }
        findings
    }

    /// Record a finding of `rule`.
    fn flag(&mut self, rule: Rule, violation: String) {
        self.findings.push((rule, violation));
    }

    /// Flag a declared name that names a private walk.
    fn check_declared_name(&mut self, kind: &str, name: &syn::Ident) {
        let name = name.to_string();
        if let Some(walk) = WALK_NAMES.iter().find(|walk| name.contains(**walk)) {
            self.flag(Rule::PrivateWalk, format!("declares {kind} `{name}`, named after a private walk (`{walk}`)"));
        }
    }
}

impl<'ast> Visit<'ast> for LinkedRetrievalVisitor {
    fn visit_item(&mut self, item: &'ast Item) {
        if item_is_test_only(item) {
            return;
        }
        visit_item(self, item);
    }

    fn visit_item_impl(&mut self, item: &'ast ItemImpl) {
        let trait_name = item
            .trait_
            .as_ref()
            .and_then(|(path, _)| path.segments.last())
            .map(|segment| segment.ident.to_string());
        if let Some(adapter) = trait_name.as_deref().filter(|name| ADAPTER_TRAITS.contains(name)) {
            self.flag(Rule::Adapter, format!("implements `{adapter}` outside the registered adapters"));
        }
        let converts_admission = is_admission_conversion(item);
        self.admission_conversion_depth += usize::from(converts_admission);
        visit_item_impl(self, item);
        self.admission_conversion_depth -= usize::from(converts_admission);
    }

    fn visit_item_fn(&mut self, item: &'ast ItemFn) {
        self.check_declared_name("function", &item.sig.ident);
        visit_item_fn(self, item);
    }

    fn visit_impl_item_fn(&mut self, item: &'ast ImplItemFn) {
        self.check_declared_name("method", &item.sig.ident);
        visit_impl_item_fn(self, item);
    }

    fn visit_trait_item_fn(&mut self, item: &'ast TraitItemFn) {
        self.check_declared_name("method", &item.sig.ident);
        visit_trait_item_fn(self, item);
    }

    fn visit_item_struct(&mut self, item: &'ast ItemStruct) {
        self.check_declared_name("struct", &item.ident);
        visit_item_struct(self, item);
    }

    fn visit_item_enum(&mut self, item: &'ast ItemEnum) {
        self.check_declared_name("enum", &item.ident);
        visit_item_enum(self, item);
    }

    fn visit_item_type(&mut self, item: &'ast ItemType) {
        self.check_declared_name("type", &item.ident);
        visit_item_type(self, item);
    }

    fn visit_item_trait(&mut self, item: &'ast ItemTrait) {
        self.check_declared_name("trait", &item.ident);
        visit_item_trait(self, item);
    }

    fn visit_pat(&mut self, pattern: &'ast Pat) {
        self.pattern_depth += 1;
        visit_pat(self, pattern);
        self.pattern_depth -= 1;
    }

    fn visit_path(&mut self, path: &'ast syn::Path) {
        let segments = path_segments(path);
        if self.pattern_depth > 0 && names_retrieving_mode(&segments) {
            self.flag(Rule::JoinModeMatch, format!("matches on `{}`", segments.join("::")));
        }
        visit_path(self, path);
    }

    fn visit_expr_path(&mut self, expression: &'ast ExprPath) {
        let segments = path_segments(&expression.path);
        if let Some(read) = segments.last().filter(|last| NESTED_TERM_READS.contains(&last.as_str())) {
            self.flag(Rule::NestedTerm, format!("reads a nested projection term through `{read}`"));
        }
        if let [.., owner, constructor] = segments.as_slice()
            && owner == "AttributeIriIndex"
            && constructor == "new"
        {
            self.builds_index = true;
            self.flag(Rule::IndexBuild, "builds an `AttributeIriIndex` by hand".to_owned());
        }
        if self.pattern_depth == 0 {
            self.check_admission_variant(&segments);
        }
        visit_expr_path(self, expression);
    }

    fn visit_expr_struct(&mut self, expression: &'ast ExprStruct) {
        self.check_admission_variant(&path_segments(&expression.path));
        visit_expr_struct(self, expression);
    }

    fn visit_expr_method_call(&mut self, call: &'ast ExprMethodCall) {
        let method = call.method.to_string();
        if NESTED_TERM_READS.contains(&method.as_str()) {
            self.flag(Rule::NestedTerm, format!("reads a nested projection term through `{method}`"));
        }
        if MEMBER_WRITES.contains(&method.as_str())
            && let Some(member) = call.args.first().and_then(written_inline_member)
        {
            self.flag(Rule::InlineMember, format!("writes the inline member `{member}` through `{method}`"));
        }
        if method == "insert" && call.args.len() == 2 {
            self.inserts.push("inserts into an `AttributeIriIndex` by hand".to_owned());
        }
        visit_expr_method_call(self, call);
    }

    fn visit_macro(&mut self, invocation: &'ast Macro) {
        // `matches!` takes a pattern the parser leaves as tokens.
        if invocation.path.is_ident("matches") {
            let tokens: Vec<String> = invocation.tokens.clone().into_iter().map(|token| token.to_string()).collect();
            let named = tokens.windows(4).any(|window| {
                JOIN_MODE_TYPES.contains(&window[0].as_str()) && window[1] == ":" && window[2] == ":" && RETRIEVING_MODES.contains(&window[3].as_str())
            });
            if named {
                self.flag(Rule::JoinModeMatch, "matches on a join mode inside `matches!`".to_owned());
            }
        }
        visit_macro(self, invocation);
    }
}

impl LinkedRetrievalVisitor {
    /// Flag an admission `ParameterError` variant built outside its
    /// conversion.
    fn check_admission_variant(&mut self, segments: &[String]) {
        if self.admission_conversion_depth > 0 {
            return;
        }
        if let [.., owner, variant] = segments
            && owner == "ParameterError"
            && ADMISSION_VARIANTS.contains(&variant.as_str())
        {
            self.flag(
                Rule::AdmissionError,
                format!("builds `ParameterError::{variant}` outside `From<AdmissionError>`"),
            );
        }
    }
}

/// The identifiers of a path's segments.
fn path_segments(path: &syn::Path) -> Vec<String> {
    path.segments.iter().map(|segment| segment.ident.to_string()).collect()
}

/// Whether a path names a retrieving join mode.
fn names_retrieving_mode(segments: &[String]) -> bool {
    segments
        .windows(2)
        .any(|pair| JOIN_MODE_TYPES.contains(&pair[0].as_str()) && RETRIEVING_MODES.contains(&pair[1].as_str()))
}

/// The clause 4.5.23.2 inline member a written key names, read through any
/// `to_owned`, `to_string` or `into` conversion around it.
fn written_inline_member(key: &Expr) -> Option<String> {
    // `syn::Expr` has many shapes and none but these three spells a member key
    // from the constant, so each is tested for rather than every shape matched.
    if let Expr::MethodCall(call) = key {
        return written_inline_member(&call.receiver);
    }
    if let Expr::Reference(reference) = key {
        return written_inline_member(&reference.expr);
    }
    let Expr::Path(expression) = key else {
        return None;
    };
    let segments = path_segments(&expression.path);
    let named = segments
        .windows(2)
        .any(|pair| INLINE_MEMBER_MODULES.contains(&pair[0].as_str()) && INLINE_MEMBERS.contains(&pair[1].as_str()));
    named.then(|| segments.join("::"))
}

/// Whether an impl is `From<AdmissionError> for ParameterError`.
fn is_admission_conversion(item: &ItemImpl) -> bool {
    let Some((trait_path, _)) = &item.trait_ else {
        return false;
    };
    let converts_admission_error = trait_path.segments.last().is_some_and(|segment| {
        segment.ident == "From"
            && match &segment.arguments {
                PathArguments::AngleBracketed(arguments) => arguments
                    .args
                    .iter()
                    .any(|argument| quote::ToTokens::to_token_stream(argument).to_string() == "AdmissionError"),
                PathArguments::None | PathArguments::Parenthesized(_) => false,
            }
    });
    let into_parameter_error =
        matches!(&*item.self_ty, Type::Path(target) if target.path.segments.last().is_some_and(|segment| segment.ident == "ParameterError"));
    converts_admission_error && into_parameter_error
}
