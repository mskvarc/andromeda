//! A function, as a migration declares it.

use crate::{
    declared_schema::{declared_type::DeclaredType, origin::Origin, qualified_name::QualifiedName, statement_context::StatementContext},
    plpgsql_tree::{PlpgsqlError, PlpgsqlRoutine},
};
use pg_query::{
    Node,
    NodeEnum,
    protobuf::{CreateFunctionStmt, DefElem},
};
use std::ops::Range;

/// The language of a PL/pgSQL routine.
const PLPGSQL: &str = "plpgsql";

/// The return type of a trigger function.
const TRIGGER_TYPE: &str = "trigger";

/// How a function's body is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FunctionBody {
    /// A SQL-standard body, `return ...` or `begin atomic ... end`, parsed and
    /// bound when the function is created.
    SqlStandard,
    /// A string constant, parsed every time the function is first called in a
    /// session, against that session's search path.
    Literal,
}

/// One `create function` or `create procedure`.
#[derive(Debug)]
pub struct DeclaredFunction {
    /// The function's name.
    name: QualifiedName,
    /// The language its body is written in.
    language: String,
    /// How its body is written.
    body: FunctionBody,
    /// Whether it is created `or replace`.
    or_replace: bool,
    /// Its return type, absent for a procedure.
    returns: Option<DeclaredType>,
    /// The PL/pgSQL tree of its body, for a PL/pgSQL function.
    routines: Vec<PlpgsqlRoutine>,
    /// Where its name is written.
    origin: Origin,
    /// The bytes of the migration holding its definition after its name.
    definition: Range<usize>,
}

impl DeclaredFunction {
    /// The function `function` declares, read from `statement`.
    ///
    /// # Errors
    ///
    /// Returns [`PlpgsqlError`] when the body of a PL/pgSQL function cannot be
    /// read.
    pub fn read(function: &CreateFunctionStmt, statement: &StatementContext<'_>) -> Result<Option<DeclaredFunction>, PlpgsqlError> {
        let Some(name) = QualifiedName::of_nodes(&function.funcname) else {
            return Ok(None);
        };
        let options = function.options.iter().filter_map(definition_element).collect::<Vec<_>>();
        let language = options
            .iter()
            .find(|option| option.defname == "language")
            .and_then(|option| string_argument(option))
            .unwrap_or_default()
            .to_owned();
        let returns = function.return_type.as_ref().and_then(DeclaredType::of);
        let routines = if language == PLPGSQL {
            let start = statement.span().start;
            let trigger_return = function
                .return_type
                .as_ref()
                .filter(|_| returns.as_ref().is_some_and(|returns| returns.name().name() == TRIGGER_TYPE))
                .and_then(|returns| usize::try_from(returns.location).ok())
                .map(|location| location.saturating_sub(start)..location.saturating_sub(start) + TRIGGER_TYPE.len());
            PlpgsqlRoutine::parse(statement.text(), trigger_return)?
        } else {
            Vec::new()
        };
        let origin = statement.origin_of_name(name.name());
        Ok(Some(DeclaredFunction {
            definition: statement.rest_from(statement.after_name(origin.offset(), name.name())),
            body: if function.sql_body.is_some() {
                FunctionBody::SqlStandard
            } else {
                FunctionBody::Literal
            },
            or_replace: function.replace,
            name,
            language,
            returns,
            routines,
            origin,
        }))
    }

    /// The function's name.
    #[must_use]
    pub const fn name(&self) -> &QualifiedName {
        &self.name
    }

    /// The language its body is written in.
    #[must_use]
    pub fn language(&self) -> &str {
        &self.language
    }

    /// How its body is written.
    #[must_use]
    pub const fn body(&self) -> FunctionBody {
        self.body
    }

    /// Whether it is created `or replace`.
    #[must_use]
    pub const fn is_or_replace(&self) -> bool {
        self.or_replace
    }

    /// Its return type, absent for a procedure.
    #[must_use]
    pub const fn returns(&self) -> Option<&DeclaredType> {
        self.returns.as_ref()
    }

    /// The PL/pgSQL tree of its body, for a PL/pgSQL function.
    #[must_use]
    pub fn routines(&self) -> &[PlpgsqlRoutine] {
        &self.routines
    }

    /// Where its name is written.
    #[must_use]
    pub const fn origin(&self) -> Origin {
        self.origin
    }

    /// The bytes of the migration holding its definition after its name.
    #[must_use]
    pub fn definition(&self) -> Range<usize> {
        self.definition.clone()
    }
}

/// The definition element `node` holds.
#[must_use]
pub fn definition_element(node: &Node) -> Option<&DefElem> {
    match &node.node {
        Some(NodeEnum::DefElem(element)) => Some(element),
        Some(_) | None => None,
    }
}

/// The argument of `element`.
fn argument(element: &DefElem) -> Option<&NodeEnum> {
    element.arg.as_ref().and_then(|argument| argument.node.as_ref())
}

/// The string argument of `element`, when it has one.
#[must_use]
pub fn string_argument(element: &DefElem) -> Option<&str> {
    let NodeEnum::String(string) = argument(element)? else {
        return None;
    };
    Some(string.sval.as_str())
}
