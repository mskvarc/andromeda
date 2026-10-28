//! The block structure of a PL/pgSQL routine: its outermost block, nested
//! blocks with their exception handlers, and the SQL expressions every node
//! embeds.

use crate::plpgsql_tree::{Collected, statement::Statement};
use serde::Deserialize;

/// The node of a routine.
#[derive(Debug, Deserialize)]
pub(super) struct FunctionNode {
    /// The routine's outermost block.
    pub(super) action: BlockNode,
}

/// A block wrapped in its node name.
#[derive(Debug, Deserialize)]
pub(super) struct BlockNode {
    /// The block.
    #[serde(rename = "PLpgSQL_stmt_block")]
    pub(super) block: Block,
}

/// `begin ... exception ... end`.
#[derive(Debug, Deserialize)]
pub(super) struct Block {
    /// The statements of the block.
    #[serde(default)]
    pub(super) body: Vec<Statement>,
    /// The exception handlers, when the block has any.
    pub(super) exceptions: Option<ExceptionBlockNode>,
}

impl Block {
    /// Gather from the block and every statement it nests.
    pub(super) fn collect<'tree>(&'tree self, collected: &mut Collected<'tree, '_>) {
        collect_all(&self.body, collected);
        for handler in self.exceptions.iter().flat_map(|exceptions| &exceptions.block.exc_list) {
            collected.handler(handler);
            collect_all(&handler.exception.action, collected);
        }
    }
}

/// Gather from every statement of `statements`.
pub(super) fn collect_all<'tree>(statements: &'tree [Statement], collected: &mut Collected<'tree, '_>) {
    for statement in statements {
        statement.collect(collected);
    }
}

/// The exception handlers of a block, wrapped in their node name.
#[derive(Debug, Deserialize)]
pub(super) struct ExceptionBlockNode {
    /// The handlers.
    #[serde(rename = "PLpgSQL_exception_block")]
    pub(super) block: ExceptionBlock,
}

/// The handlers of a block.
#[derive(Debug, Deserialize)]
pub(super) struct ExceptionBlock {
    /// One handler per `when` clause.
    #[serde(default)]
    pub(super) exc_list: Vec<ExceptionNode>,
}

/// One handler, wrapped in its node name.
#[derive(Debug, Deserialize)]
pub(super) struct ExceptionNode {
    /// The handler.
    #[serde(rename = "PLpgSQL_exception")]
    pub(super) exception: Exception,
}

/// `when <conditions> then <action>`.
#[derive(Debug, Deserialize)]
pub(super) struct Exception {
    /// The conditions the handler catches.
    #[serde(default)]
    pub(super) conditions: Vec<ConditionNode>,
    /// The statements it runs.
    #[serde(default)]
    pub(super) action: Vec<Statement>,
}

/// One caught condition, wrapped in its node name.
#[derive(Debug, Deserialize)]
pub(super) struct ConditionNode {
    /// The condition.
    #[serde(rename = "PLpgSQL_condition")]
    pub(super) condition: Condition,
}

/// A condition named by a handler.
#[derive(Debug, Deserialize)]
pub(super) struct Condition {
    /// Its name as PostgreSQL knows it, `others` for the catch-all.
    #[serde(default)]
    pub(super) condname: String,
}

/// An SQL expression, wrapped in its node name.
#[derive(Debug, Deserialize)]
pub(super) struct ExpressionNode {
    /// The expression.
    #[serde(rename = "PLpgSQL_expr")]
    pub(super) expression: Expression,
}

/// An SQL expression embedded in PL/pgSQL.
#[derive(Debug, Deserialize)]
pub(super) struct Expression {
    /// The text as written, with any `into` clause blanked out.
    pub(super) query: String,
    /// The grammar the text is written in, as a parser option.
    #[serde(rename = "parseMode", default)]
    pub(super) parse_mode: i32,
}
