//! The syntax walk that finds, in one parsed source, each mark a second copy
//! of registration or subscription matching leaves.

use crate::{
    registration_matching::{
        rule::Rule,
        selector_bindings::SelectorBindings,
        token_reading::{is_order_comparison, names_active_status, names_current_instant, order_comparison, reads_expiry_field, reads_owned_expiry},
    },
    source_scan::{collect_use_paths, item_is_test_only},
};
use proc_macro2::{LineColumn, Span};
use quote::ToTokens;
use syn::{
    BinOp,
    Expr,
    ExprAssign,
    ExprBinary,
    ExprField,
    ExprForLoop,
    ExprMethodCall,
    ExprPath,
    ImplItemFn,
    Item,
    ItemFn,
    ItemUse,
    Local,
    Macro,
    Member,
    Pat,
    Signature,
    spanned::Spanned,
    visit::{
        Visit,
        visit_expr,
        visit_expr_binary,
        visit_expr_field,
        visit_expr_for_loop,
        visit_expr_method_call,
        visit_expr_path,
        visit_impl_item_fn,
        visit_item,
        visit_item_fn,
        visit_item_use,
        visit_local,
        visit_macro,
    },
};

/// The words an imported path segment carries when the source handles
/// registrations or subscriptions.
const MATCHED_RESOURCES: [&str; 3] = ["registration", "subscription", "entity_info"];

/// The methods that match a type expression.
const TYPE_MATCHES: [&str; 2] = ["admits", "matches"];

/// One decision found in a source.
pub(crate) struct Finding {
    /// The rule the decision belongs to.
    pub(crate) rule: Rule,
    /// Where the decision starts.
    pub(crate) start: LineColumn,
    /// What the decision is.
    pub(crate) subject: String,
}

/// Collects what one parsed source decides about registration or
/// subscription matching.
#[derive(Default)]
pub(crate) struct RegistrationMatchingVisitor {
    /// Whether the source imports a registration or subscription type.
    handles_matched_resources: bool,
    /// Whether the source imports the subscription `EntitySelector`, so that
    /// the name stands for a selector of a subscription.
    imports_subscription_selector: bool,
    /// The selector bindings of the functions enclosing the current node,
    /// innermost last.
    functions: Vec<SelectorBindings>,
    /// Findings that are decisions only in a source handling registrations
    /// or subscriptions.
    contextual: Vec<Finding>,
    /// Findings that hold whatever the source imports.
    findings: Vec<Finding>,
}

impl RegistrationMatchingVisitor {
    /// Every finding this source commits, in source order.
    pub(crate) fn into_findings(self) -> Vec<Finding> {
        let mut findings = self.findings;
        if self.handles_matched_resources {
            findings.extend(self.contextual);
        }
        findings.sort_by_key(|finding| (finding.start.line, finding.start.column));
        findings
    }

    /// Record a finding of `rule` starting at `span`.
    fn flag(&mut self, rule: Rule, span: Span, subject: &str) {
        self.findings.push(finding(rule, span, subject));
    }

    /// Record a finding of `rule` starting at `span` that holds only in a
    /// source handling registrations or subscriptions.
    fn flag_in_context(&mut self, rule: Rule, span: Span, subject: &str) {
        self.contextual.push(finding(rule, span, subject));
    }

    /// Visit one function in a binding frame of its own.
    fn within_function(&mut self, signature: &Signature, visit_function: impl FnOnce(&mut RegistrationMatchingVisitor)) {
        self.functions
            .push(SelectorBindings::of_parameters(signature, self.imports_subscription_selector));
        visit_function(self);
        self.functions.pop();
    }

    /// Flag an `expires_at` compared inside a closure handed to a method
    /// called on it, such as `expires_at.is_none_or(|at| at > now)`.
    fn flag_expiry_closure(&mut self, call: &ExprMethodCall, span: Span) {
        let compared: Vec<_> = call.args.iter().filter_map(order_comparison).collect();
        if compared.is_empty() {
            return;
        }
        let receiver = call.receiver.to_token_stream();
        if reads_owned_expiry(&receiver) {
            self.flag(Rule::Expiry, span, "compares a registration or subscription `expires_at` through a closure");
        } else if reads_expiry_field(&receiver) && compared.iter().any(names_current_instant) {
            self.flag_in_context(Rule::Expiry, span, "compares an `expires_at` with the current instant through a closure");
        }
    }

    /// Hold the names `pattern` binds when `source` yields a selector.
    fn bind_when_selector(&mut self, source: &Expr, pattern: &Pat) {
        if let Some(bindings) = self.functions.last_mut()
            && bindings.yields_selector(source)
        {
            bindings.bind(pattern);
        }
    }
}

impl<'ast> Visit<'ast> for RegistrationMatchingVisitor {
    fn visit_item(&mut self, item: &'ast Item) {
        if item_is_test_only(item) {
            return;
        }
        visit_item(self, item);
    }

    fn visit_item_use(&mut self, item: &'ast ItemUse) {
        let mut prefix = Vec::new();
        collect_use_paths(&item.tree, &mut prefix, &mut |segments| {
            if segments.iter().any(|segment| names_matched_resource(segment)) {
                self.handles_matched_resources = true;
            }
            if segments.last().is_some_and(|leaf| leaf == "EntitySelector") && segments.iter().any(|segment| segment == "subscription") {
                self.imports_subscription_selector = true;
            }
        });
        visit_item_use(self, item);
    }

    fn visit_item_fn(&mut self, item: &'ast ItemFn) {
        self.within_function(&item.sig, |visitor| visit_item_fn(visitor, item));
    }

    fn visit_impl_item_fn(&mut self, item: &'ast ImplItemFn) {
        self.within_function(&item.sig, |visitor| visit_impl_item_fn(visitor, item));
    }

    fn visit_local(&mut self, local: &'ast Local) {
        if let Some(init) = &local.init {
            self.bind_when_selector(&init.expr, &local.pat);
        }
        visit_local(self, local);
    }

    fn visit_expr_for_loop(&mut self, expression: &'ast ExprForLoop) {
        self.bind_when_selector(&expression.expr, &expression.pat);
        visit_expr_for_loop(self, expression);
    }

    fn visit_expr_assign(&mut self, expression: &'ast ExprAssign) {
        // Writing a field spells a selection; only the target's owner is read.
        if let Expr::Field(target) = &*expression.left {
            visit_expr(self, &target.base);
        } else {
            visit_expr(self, &expression.left);
        }
        visit_expr(self, &expression.right);
    }

    fn visit_expr_field(&mut self, expression: &'ast ExprField) {
        let span = expression.member.span();
        if member_is(&expression.member, "id_pattern") && self.functions.last().is_some_and(|bindings| bindings.holds(&expression.base)) {
            self.flag_in_context(
                Rule::RegisteredIdentifier,
                span,
                "reads a registered `id_pattern` to decide an identifier match",
            );
        }
        if member_is(&expression.member, "scope") && matches!(&*expression.base, Expr::Field(base) if member_is(&base.member, "coverage")) {
            self.flag(Rule::ScopeCoverage, span, "reads `coverage.scope` to decide a Scope match");
        }
        visit_expr_field(self, expression);
    }

    fn visit_expr_method_call(&mut self, call: &'ast ExprMethodCall) {
        let method = call.method.to_string();
        let span = call.method.span();
        if method == "is_match" {
            self.flag_in_context(Rule::RegisteredIdentifier, span, "calls `is_match` to decide an identifier match");
        }
        if TYPE_MATCHES.contains(&method.as_str()) && matches!(&*call.receiver, Expr::Field(field) if member_is(&field.member, "type_")) {
            self.flag(Rule::RegisteredType, span, &format!("calls `type_.{method}` to decide a type match"));
        }
        self.flag_expiry_closure(call, span);
        for argument in &call.args {
            if let Expr::Closure(closure) = argument {
                closure.inputs.iter().for_each(|input| self.bind_when_selector(&call.receiver, input));
            }
        }
        visit_expr_method_call(self, call);
    }

    fn visit_expr_path(&mut self, expression: &'ast ExprPath) {
        let segments: Vec<String> = expression.path.segments.iter().map(|segment| segment.ident.to_string()).collect();
        if let [.., owner, constructor] = segments.as_slice() {
            if owner == "ContextSourceProperties" {
                self.flag(
                    Rule::PropertyView,
                    expression.span(),
                    &format!("builds a `csf` view through `ContextSourceProperties::{constructor}`"),
                );
            }
            if owner == "IdentifierAxis" && constructor == "of" {
                self.flag(Rule::SelectorAxis, expression.span(), "builds an identifier axis through `IdentifierAxis::of`");
            }
        }
        visit_expr_path(self, expression);
    }

    fn visit_expr_binary(&mut self, expression: &'ast ExprBinary) {
        let span = expression.span();
        let (left, right) = (expression.left.to_token_stream(), expression.right.to_token_stream());
        if is_order_comparison(expression.op) {
            if reads_owned_expiry(&left) || reads_owned_expiry(&right) {
                self.flag(Rule::Expiry, span, "compares a registration or subscription `expires_at`");
            } else if (reads_expiry_field(&left) && names_current_instant(&right)) || (reads_expiry_field(&right) && names_current_instant(&left)) {
                self.flag_in_context(Rule::Expiry, span, "compares an `expires_at` with the current instant");
            }
        }
        if matches!(expression.op, BinOp::Eq(_) | BinOp::Ne(_)) && (names_active_status(&left) || names_active_status(&right)) {
            self.flag(Rule::Standing, span, "compares with `SubscriptionStatus::Active` to decide standing");
        }
        visit_expr_binary(self, expression);
    }

    fn visit_macro(&mut self, invocation: &'ast Macro) {
        // `matches!` takes a pattern the parser leaves as tokens.
        if invocation.path.is_ident("matches") && names_active_status(&invocation.tokens) {
            self.flag(Rule::Standing, invocation.span(), "matches `SubscriptionStatus::Active` to decide standing");
        }
        visit_macro(self, invocation);
    }
}

/// A finding of `rule` starting at `span`.
fn finding(rule: Rule, span: Span, subject: &str) -> Finding {
    Finding {
        rule,
        start: span.start(),
        subject: subject.to_owned(),
    }
}

/// Whether an imported path segment names a registration or subscription
/// module or type.
fn names_matched_resource(segment: &str) -> bool {
    let lowered = segment.to_lowercase();
    MATCHED_RESOURCES.iter().any(|resource| lowered.contains(resource))
}

/// Whether a field access reads the named field.
fn member_is(member: &Member, name: &str) -> bool {
    matches!(member, Member::Named(ident) if ident == name)
}
