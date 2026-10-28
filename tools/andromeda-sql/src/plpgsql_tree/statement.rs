//! Every PL/pgSQL statement kind, each keeping the statements it nests and
//! the SQL expressions it embeds.

use crate::plpgsql_tree::{
    Collected,
    SqlRole,
    block::{Block, ExpressionNode, collect_all},
};
use serde::Deserialize;

/// Every PL/pgSQL statement kind, each keeping what a rule reads.
#[derive(Debug, Deserialize)]
pub(super) enum Statement {
    /// A nested block.
    #[serde(rename = "PLpgSQL_stmt_block")]
    Block(Block),
    /// `target := expression`.
    #[serde(rename = "PLpgSQL_stmt_assign")]
    Assign(ValueStatement),
    /// `if ... elsif ... else ... end if`.
    #[serde(rename = "PLpgSQL_stmt_if")]
    If(IfStatement),
    /// `case ... when ... end case`.
    #[serde(rename = "PLpgSQL_stmt_case")]
    Case(CaseStatement),
    /// `loop ... end loop`.
    #[serde(rename = "PLpgSQL_stmt_loop")]
    Loop(LoopStatement),
    /// `while ... loop`.
    #[serde(rename = "PLpgSQL_stmt_while")]
    While(LoopStatement),
    /// `for i in lower .. upper loop`.
    #[serde(rename = "PLpgSQL_stmt_fori")]
    ForInteger(ForIntegerStatement),
    /// `for row in query loop`.
    #[serde(rename = "PLpgSQL_stmt_fors")]
    ForQuery(ForQueryStatement),
    /// `for row in cursor loop`.
    #[serde(rename = "PLpgSQL_stmt_forc")]
    ForCursor(ForCursorStatement),
    /// `foreach element in array loop`.
    #[serde(rename = "PLpgSQL_stmt_foreach_a")]
    ForeachArray(ForeachStatement),
    /// `exit` or `continue`.
    #[serde(rename = "PLpgSQL_stmt_exit")]
    Exit(LoopStatement),
    /// `return`.
    #[serde(rename = "PLpgSQL_stmt_return")]
    Return(ValueStatement),
    /// `return next`.
    #[serde(rename = "PLpgSQL_stmt_return_next")]
    ReturnNext(ValueStatement),
    /// `return query`.
    #[serde(rename = "PLpgSQL_stmt_return_query")]
    ReturnQuery(ReturnQueryStatement),
    /// `raise`.
    #[serde(rename = "PLpgSQL_stmt_raise")]
    Raise(RaiseStatement),
    /// `assert`.
    #[serde(rename = "PLpgSQL_stmt_assert")]
    Assert(AssertStatement),
    /// A plain SQL statement.
    #[serde(rename = "PLpgSQL_stmt_execsql")]
    ExecuteSql(ExecuteSqlStatement),
    /// `execute <command>`.
    #[serde(rename = "PLpgSQL_stmt_dynexecute")]
    ExecuteDynamic(DynamicStatement),
    /// `for row in execute <command> loop`.
    #[serde(rename = "PLpgSQL_stmt_dynfors")]
    ForDynamic(DynamicStatement),
    /// `get diagnostics`.
    #[serde(rename = "PLpgSQL_stmt_getdiag")]
    GetDiagnostics(Empty),
    /// `open cursor`.
    #[serde(rename = "PLpgSQL_stmt_open")]
    Open(OpenStatement),
    /// `fetch` or `move`.
    #[serde(rename = "PLpgSQL_stmt_fetch")]
    Fetch(ValueStatement),
    /// `close cursor`.
    #[serde(rename = "PLpgSQL_stmt_close")]
    Close(Empty),
    /// `perform <query>`.
    #[serde(rename = "PLpgSQL_stmt_perform")]
    Perform(ValueStatement),
    /// `call <procedure>`.
    #[serde(rename = "PLpgSQL_stmt_call")]
    Call(ValueStatement),
    /// `commit`.
    #[serde(rename = "PLpgSQL_stmt_commit")]
    Commit(Empty),
    /// `rollback`.
    #[serde(rename = "PLpgSQL_stmt_rollback")]
    Rollback(Empty),
}

impl Statement {
    /// Gather from the statement and every statement it nests.
    pub(super) fn collect<'tree>(&'tree self, collected: &mut Collected<'tree, '_>) {
        match self {
            Statement::Block(block) => block.collect(collected),
            Statement::Assign(statement)
            | Statement::Return(statement)
            | Statement::ReturnNext(statement)
            | Statement::Fetch(statement)
            | Statement::Perform(statement)
            | Statement::Call(statement) => collected.expression(statement.expr.as_ref(), SqlRole::Static),
            Statement::If(statement) => statement.collect(collected),
            Statement::Case(statement) => statement.collect(collected),
            Statement::Loop(statement) | Statement::While(statement) | Statement::Exit(statement) => {
                collected.expression(statement.cond.as_ref(), SqlRole::Static);
                collect_all(&statement.body, collected);
            }
            Statement::ForInteger(statement) => {
                for bound in [&statement.lower, &statement.upper, &statement.step] {
                    collected.expression(bound.as_ref(), SqlRole::Static);
                }
                collect_all(&statement.body, collected);
            }
            Statement::ForQuery(statement) => {
                collected.expression(statement.query.as_ref(), SqlRole::Static);
                collect_all(&statement.body, collected);
            }
            Statement::ForCursor(statement) => {
                collected.expression(statement.argquery.as_ref(), SqlRole::Static);
                collect_all(&statement.body, collected);
            }
            Statement::ForeachArray(statement) => {
                collected.expression(statement.expr.as_ref(), SqlRole::Static);
                collect_all(&statement.body, collected);
            }
            Statement::ReturnQuery(statement) => {
                collected.expression(statement.query.as_ref(), SqlRole::Static);
                collected.expression(statement.dynquery.as_ref(), SqlRole::DynamicCommand);
                collected.values(&statement.params);
            }
            Statement::ExecuteDynamic(statement) | Statement::ForDynamic(statement) => {
                collected.expression(statement.query.as_ref(), SqlRole::DynamicCommand);
                collected.values(&statement.params);
                collect_all(&statement.body, collected);
            }
            Statement::Raise(statement) => {
                collected.values(&statement.params);
                for option in &statement.options {
                    collected.expression(option.option.expr.as_ref(), SqlRole::Static);
                }
            }
            Statement::Assert(statement) => {
                collected.expression(statement.cond.as_ref(), SqlRole::Static);
                collected.expression(statement.message.as_ref(), SqlRole::Static);
            }
            Statement::ExecuteSql(statement) => collected.expression(statement.sqlstmt.as_ref(), SqlRole::Static),
            Statement::Open(statement) => {
                collected.expression(statement.argquery.as_ref(), SqlRole::Static);
                collected.expression(statement.query.as_ref(), SqlRole::Static);
                collected.expression(statement.dynquery.as_ref(), SqlRole::DynamicCommand);
                collected.values(&statement.params);
            }
            Statement::GetDiagnostics(_) | Statement::Close(_) | Statement::Commit(_) | Statement::Rollback(_) => {}
        }
    }
}

/// A statement that embeds no expression.
#[derive(Debug, Deserialize)]
pub(super) struct Empty {}

/// A statement embedding at most one value expression.
#[derive(Debug, Deserialize)]
pub(super) struct ValueStatement {
    /// The expression.
    pub(super) expr: Option<ExpressionNode>,
}

/// `if`.
#[derive(Debug, Deserialize)]
pub(super) struct IfStatement {
    /// The first condition.
    pub(super) cond: Option<ExpressionNode>,
    /// What runs when it holds.
    #[serde(default)]
    pub(super) then_body: Vec<Statement>,
    /// The `elsif` branches.
    #[serde(default)]
    pub(super) elsif_list: Vec<ElsifNode>,
    /// What runs when no condition holds.
    #[serde(default)]
    pub(super) else_body: Vec<Statement>,
}

impl IfStatement {
    /// Gather from every branch.
    pub(super) fn collect<'tree>(&'tree self, collected: &mut Collected<'tree, '_>) {
        collected.expression(self.cond.as_ref(), SqlRole::Static);
        collect_all(&self.then_body, collected);
        for branch in &self.elsif_list {
            collected.expression(branch.branch.cond.as_ref(), SqlRole::Static);
            collect_all(&branch.branch.stmts, collected);
        }
        collect_all(&self.else_body, collected);
    }
}

/// One `elsif` branch, wrapped in its node name.
#[derive(Debug, Deserialize)]
pub(super) struct ElsifNode {
    /// The branch.
    #[serde(rename = "PLpgSQL_if_elsif")]
    pub(super) branch: GuardedBranch,
}

/// `case`.
#[derive(Debug, Deserialize)]
pub(super) struct CaseStatement {
    /// The tested value, for a simple `case`.
    pub(super) t_expr: Option<ExpressionNode>,
    /// The `when` branches.
    #[serde(default)]
    pub(super) case_when_list: Vec<CaseWhenNode>,
    /// What runs when no branch matches.
    #[serde(default)]
    pub(super) else_stmts: Vec<Statement>,
}

impl CaseStatement {
    /// Gather from every branch.
    pub(super) fn collect<'tree>(&'tree self, collected: &mut Collected<'tree, '_>) {
        collected.expression(self.t_expr.as_ref(), SqlRole::Static);
        for branch in &self.case_when_list {
            collected.expression(branch.branch.expr.as_ref(), SqlRole::Static);
            collect_all(&branch.branch.stmts, collected);
        }
        collect_all(&self.else_stmts, collected);
    }
}

/// One `when` branch of a `case`, wrapped in its node name.
#[derive(Debug, Deserialize)]
pub(super) struct CaseWhenNode {
    /// The branch.
    #[serde(rename = "PLpgSQL_case_when")]
    pub(super) branch: CaseWhen,
}

/// `when <expression> then <statements>`.
#[derive(Debug, Deserialize)]
pub(super) struct CaseWhen {
    /// The tested expression.
    pub(super) expr: Option<ExpressionNode>,
    /// What runs when it matches.
    #[serde(default)]
    pub(super) stmts: Vec<Statement>,
}

/// `elsif <condition> then <statements>`.
#[derive(Debug, Deserialize)]
pub(super) struct GuardedBranch {
    /// The condition.
    pub(super) cond: Option<ExpressionNode>,
    /// What runs when it holds.
    #[serde(default)]
    pub(super) stmts: Vec<Statement>,
}

/// A loop, a `while` or an `exit`: an optional condition and an optional body.
#[derive(Debug, Deserialize)]
pub(super) struct LoopStatement {
    /// The condition, when there is one.
    pub(super) cond: Option<ExpressionNode>,
    /// The statements repeated.
    #[serde(default)]
    pub(super) body: Vec<Statement>,
}

/// `for i in lower .. upper by step loop`.
#[derive(Debug, Deserialize)]
pub(super) struct ForIntegerStatement {
    /// The lower bound.
    pub(super) lower: Option<ExpressionNode>,
    /// The upper bound.
    pub(super) upper: Option<ExpressionNode>,
    /// The step.
    pub(super) step: Option<ExpressionNode>,
    /// The statements repeated.
    #[serde(default)]
    pub(super) body: Vec<Statement>,
}

/// `for row in query loop`.
#[derive(Debug, Deserialize)]
pub(super) struct ForQueryStatement {
    /// The query.
    pub(super) query: Option<ExpressionNode>,
    /// The statements repeated.
    #[serde(default)]
    pub(super) body: Vec<Statement>,
}

/// `for row in cursor (arguments) loop`.
#[derive(Debug, Deserialize)]
pub(super) struct ForCursorStatement {
    /// The cursor arguments.
    pub(super) argquery: Option<ExpressionNode>,
    /// The statements repeated.
    #[serde(default)]
    pub(super) body: Vec<Statement>,
}

/// `foreach element in array loop`.
#[derive(Debug, Deserialize)]
pub(super) struct ForeachStatement {
    /// The array.
    pub(super) expr: Option<ExpressionNode>,
    /// The statements repeated.
    #[serde(default)]
    pub(super) body: Vec<Statement>,
}

/// `return query`, static or `execute`d.
#[derive(Debug, Deserialize)]
pub(super) struct ReturnQueryStatement {
    /// The static query.
    pub(super) query: Option<ExpressionNode>,
    /// The text of the command `return query execute` runs.
    pub(super) dynquery: Option<ExpressionNode>,
    /// The `using` parameters.
    #[serde(default)]
    pub(super) params: Vec<ExpressionNode>,
}

/// `execute <command>` or `for row in execute <command> loop`.
#[derive(Debug, Deserialize)]
pub(super) struct DynamicStatement {
    /// The text of the command executed.
    pub(super) query: Option<ExpressionNode>,
    /// The `using` parameters.
    #[serde(default)]
    pub(super) params: Vec<ExpressionNode>,
    /// The statements repeated, for `for ... in execute`.
    #[serde(default)]
    pub(super) body: Vec<Statement>,
}

/// `raise`.
#[derive(Debug, Deserialize)]
pub(super) struct RaiseStatement {
    /// The values of the message's placeholders.
    #[serde(default)]
    pub(super) params: Vec<ExpressionNode>,
    /// The `using` options.
    #[serde(default)]
    pub(super) options: Vec<RaiseOptionNode>,
}

/// One `using` option of a `raise`, wrapped in its node name.
#[derive(Debug, Deserialize)]
pub(super) struct RaiseOptionNode {
    /// The option.
    #[serde(rename = "PLpgSQL_raise_option")]
    pub(super) option: ValueStatement,
}

/// `assert`.
#[derive(Debug, Deserialize)]
pub(super) struct AssertStatement {
    /// The asserted condition.
    pub(super) cond: Option<ExpressionNode>,
    /// The message.
    pub(super) message: Option<ExpressionNode>,
}

/// A plain SQL statement.
#[derive(Debug, Deserialize)]
pub(super) struct ExecuteSqlStatement {
    /// The statement.
    pub(super) sqlstmt: Option<ExpressionNode>,
}

/// `open cursor`.
#[derive(Debug, Deserialize)]
pub(super) struct OpenStatement {
    /// The cursor arguments.
    pub(super) argquery: Option<ExpressionNode>,
    /// The static query.
    pub(super) query: Option<ExpressionNode>,
    /// The dynamic command.
    pub(super) dynquery: Option<ExpressionNode>,
    /// The `using` parameters.
    #[serde(default)]
    pub(super) params: Vec<ExpressionNode>,
}
