//! One parser node read as the kind of node it holds.

use pg_query::{
    Node,
    NodeEnum,
    protobuf::{FuncCall, a_const::Val},
};

/// The string constant `node` holds.
#[must_use]
pub const fn string_constant(node: &Node) -> Option<&str> {
    let Some(NodeEnum::AConst(constant)) = &node.node else {
        return None;
    };
    let Some(Val::Sval(string)) = &constant.val else {
        return None;
    };
    Some(string.sval.as_str())
}

/// The function call `node` holds.
#[must_use]
pub fn function_call(node: &Node) -> Option<&FuncCall> {
    match &node.node {
        Some(NodeEnum::FuncCall(call)) => Some(call),
        Some(_) | None => None,
    }
}
