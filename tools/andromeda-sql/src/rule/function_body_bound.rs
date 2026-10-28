//! `function-body-bound`: a function's body means the same whatever search
//! path it runs under.
//!
//! A `language sql` function written as a string is parsed when a session
//! first calls it, against that session's search path; `pg_dump` restores
//! with an empty one, and maintenance runs with `pg_catalog, pg_temp`, so a
//! generated column computed through it fails to restore. A SQL-standard body
//! (`return ...` or `begin atomic ... end`) is parsed and bound once, when the
//! function is created. A PL/pgSQL function is always parsed late, so it names
//! every relation schema-qualified, or, for the branch relation of the schema
//! the trigger fires in, through `format('%I. ...', tg_table_schema)`. The
//! transition tables a trigger declares are named by the trigger, not by a
//! schema.

use crate::{
    declared_schema::{
        DeclaredSchema,
        function::{DeclaredFunction, FunctionBody},
        parser_node::string_constant,
        syntax_walk::SyntaxSites,
        trigger::DeclaredTrigger,
    },
    plpgsql_tree::{EmbeddedSql, PlpgsqlRoutine, SqlRole},
    rule::{Rule, RuleContext, RuleFinding},
};
use pg_query::{Node, NodeEnum, ParserOptions, protobuf::FuncCall};

/// The language of a function a body string holds.
const SQL_LANGUAGE: &str = "sql";

/// The language of a PL/pgSQL function.
const PLPGSQL_LANGUAGE: &str = "plpgsql";

/// The function a dynamic command is built with.
const FORMAT: &str = "format";

/// The trigger variable naming the schema of the table a trigger fires on.
const TRIGGER_SCHEMA: &str = "tg_table_schema";

/// What an `%I` bound to anything but [`TRIGGER_SCHEMA`] reads as.
const UNBOUND_IDENTIFIER: &str = "unbound_identifier";

/// What the rule finds.
#[derive(Debug, derive_more::Display)]
enum BodyViolation<'schema> {
    /// A SQL function's body is a string.
    #[display("{_0} is a sql function with a string body, parsed against each caller's search path")]
    StringBody(&'schema str),
    /// A PL/pgSQL function names a relation without a schema.
    #[display("{function} names the relation {relation} without a schema")]
    Unqualified {
        /// The function.
        function: &'schema str,
        /// The relation.
        relation: String,
    },
    /// An expression of a PL/pgSQL function cannot be read.
    #[display("{function}: {fault}")]
    Unreadable {
        /// The function.
        function: &'schema str,
        /// What is wrong with the expression.
        fault: BodyFault,
    },
}

/// Why an expression of a PL/pgSQL body cannot be checked.
#[derive(Debug, derive_more::Display)]
enum BodyFault {
    /// The expression does not parse.
    #[display("the embedded statement `{query}` does not parse: {source}")]
    Unparsed {
        /// The expression.
        query: String,
        /// What the parser says.
        source: pg_query::Error,
    },
    /// The expression's tree could not be walked.
    #[display("the embedded statement could not be walked: {_0}")]
    Unwalkable(serde_json::Error),
    /// A dynamic command is not built by `format` from a constant template.
    #[display("the dynamic command `{_0}` is not built by format with a constant template")]
    NotFormatted(String),
}

/// Every string-bodied SQL function, and every relation a PL/pgSQL function
/// names without a schema.
#[must_use]
pub fn check(context: &RuleContext<'_>) -> Vec<RuleFinding> {
    let schema = context.schema();
    schema
        .functions()
        .iter()
        .flat_map(|function| {
            let violations = match function.language() {
                SQL_LANGUAGE if function.body() == FunctionBody::Literal => vec![BodyViolation::StringBody(function.name().name())],
                PLPGSQL_LANGUAGE => unqualified_relations(schema, function),
                _ => Vec::new(),
            };
            violations
                .iter()
                .map(|violation| context.finding(Rule::FunctionBodyBound, function.origin(), function.name().name().len(), violation))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// What is wrong with the relations the PL/pgSQL `function` names.
fn unqualified_relations<'schema>(schema: &'schema DeclaredSchema, function: &'schema DeclaredFunction) -> Vec<BodyViolation<'schema>> {
    let name = function.name().name();
    let transition_tables = schema
        .triggers()
        .iter()
        .filter(|trigger| trigger.function().name() == name)
        .flat_map(DeclaredTrigger::transition_tables)
        .map(String::as_str)
        .collect::<Vec<_>>();
    function
        .routines()
        .iter()
        .flat_map(PlpgsqlRoutine::embedded_sql)
        .flat_map(|sql| match relations_named(&sql) {
            Ok(relations) => relations
                .into_iter()
                .filter(|(qualifier, relation)| qualifier.is_none() && !transition_tables.contains(&relation.as_str()))
                .map(|(_, relation)| BodyViolation::Unqualified { function: name, relation })
                .collect(),
            Err(fault) => vec![BodyViolation::Unreadable { function: name, fault }],
        })
        .collect()
}

/// The relations `sql` names, each with its qualifier, `None` when it has
/// none or only an `%I` bound to something other than `tg_table_schema`.
fn relations_named(sql: &EmbeddedSql<'_>) -> Result<Vec<(Option<String>, String)>, BodyFault> {
    let (text, options) = match sql.role() {
        SqlRole::Static => (sql.query().to_owned(), sql.parse_options()),
        SqlRole::DynamicCommand => (expanded_command(sql.query())?, ParserOptions::DEFAULT),
    };
    let parsed = pg_query::parse(&text, options).map_err(|source| BodyFault::Unparsed {
        query: sql.query().to_owned(),
        source,
    })?;
    let mut relations = Vec::new();
    for statement in parsed.protobuf.stmts.iter().filter_map(|statement| statement.stmt.as_deref()) {
        let sites = SyntaxSites::of(statement).map_err(BodyFault::Unwalkable)?;
        relations.extend(
            sites
                .relations()
                .iter()
                .filter(|relation| relation.name().schema().is_some() || !sites.common_tables().iter().any(|common| common == relation.name().name()))
                .map(|relation| {
                    let qualifier = relation.name().schema().filter(|schema| *schema != UNBOUND_IDENTIFIER).map(str::to_owned);
                    (qualifier, relation.name().name().to_owned())
                }),
        );
    }
    Ok(relations)
}

/// The command a `format(template, ...)` call builds, every `%I` bound to
/// `tg_table_schema` written as that name, any other `%I` as
/// [`UNBOUND_IDENTIFIER`], and every `%s` and `%L` as `null`.
fn expanded_command(expression: &str) -> Result<String, BodyFault> {
    let parsed = pg_query::parse(expression, ParserOptions::PLPGSQL_EXPR).map_err(|source| BodyFault::Unparsed {
        query: expression.to_owned(),
        source,
    })?;
    let call = parsed
        .protobuf
        .stmts
        .first()
        .and_then(|statement| statement.stmt.as_deref())
        .and_then(|statement| match &statement.node {
            Some(NodeEnum::SelectStmt(select)) => select.target_list.first(),
            Some(_) | None => None,
        })
        .and_then(|target| match &target.node {
            Some(NodeEnum::ResTarget(target)) => target.val.as_deref(),
            Some(_) | None => None,
        })
        .and_then(format_call);
    let template = call.and_then(|call| call.args.first()).and_then(string_constant);
    match (call, template) {
        (Some(call), Some(template)) => Ok(expand(template, call.args.get(1..).unwrap_or_default())),
        (Some(_) | None, _) => Err(BodyFault::NotFormatted(expression.to_owned())),
    }
}

/// `template` with each placeholder replaced as [`expanded_command`] says,
/// `arguments` the values after the template.
fn expand(template: &str, arguments: &[Node]) -> String {
    let mut expanded = String::with_capacity(template.len());
    let mut next_argument = 0;
    let mut characters = template.chars().peekable();
    while let Some(character) = characters.next() {
        if character != '%' {
            expanded.push(character);
            continue;
        }
        let mut digits = String::new();
        while let Some(digit) = characters.next_if(char::is_ascii_digit) {
            digits.push(digit);
        }
        let position = if characters.next_if_eq(&'$').is_some() {
            digits.parse::<usize>().ok().map(|position| position.saturating_sub(1))
        } else {
            None
        };
        match characters.next() {
            Some('%') => expanded.push('%'),
            Some(kind) => {
                let argument = position.unwrap_or(next_argument);
                next_argument = argument + 1;
                match kind {
                    'I' if arguments.get(argument).is_some_and(names_trigger_schema) => expanded.push_str(TRIGGER_SCHEMA),
                    'I' => expanded.push_str(UNBOUND_IDENTIFIER),
                    _ => expanded.push_str("null"),
                }
            }
            None => {}
        }
    }
    expanded
}

/// The `format` call `node` holds.
fn format_call(node: &Node) -> Option<&FuncCall> {
    let Some(NodeEnum::FuncCall(call)) = &node.node else {
        return None;
    };
    let named_format = call
        .funcname
        .last()
        .is_some_and(|name| matches!(&name.node, Some(NodeEnum::String(name)) if name.sval == FORMAT));
    named_format.then_some(call.as_ref())
}

/// Whether `node` is the bare variable `tg_table_schema`.
fn names_trigger_schema(node: &Node) -> bool {
    let Some(NodeEnum::ColumnRef(column)) = &node.node else {
        return false;
    };
    matches!(column.fields.as_slice(), [field] if matches!(&field.node, Some(NodeEnum::String(name)) if name.sval == TRIGGER_SCHEMA))
}

#[cfg(test)]
mod tests {
    use crate::rule::{
        Rule,
        rule_fixture::{FixtureError, findings},
    };
    use pretty_assertions::assert_eq;

    const FILE: &str = "branch/0001_functions.sql";

    #[test]
    fn a_sql_standard_body_and_qualified_plpgsql_relations_are_accepted() -> Result<(), FixtureError> {
        let sql = "create table entities (\n    entity_key bigint not null\n);\n\ncreate function geom_to_geog (g geometry)\n    returns geography\n    language sql\n    immutable strict parallel safe\n    return g::geography;\n\ncreate function cascade_entities ()\n    returns trigger\n    language plpgsql\n    as $$\nbegin\n    execute format('delete from %I.entities where entity_key = any ($1)', tg_table_schema)\n    using array(select deleted.entity_key from old_rows deleted);\n    perform count(*) from public.entities;\n    return null;\nend\n$$;\n\ncreate trigger trg_entities_cascade\n    after delete on entities referencing old table as old_rows for each statement\n    execute function cascade_entities ();\n";
        assert_eq!(findings(Rule::FunctionBodyBound, &[(FILE, sql)])?, Vec::<String>::new());
        Ok(())
    }

    #[test]
    fn a_string_body_sql_function_is_refused_at_its_name() -> Result<(), FixtureError> {
        let sql = "create function geom_to_geog (g geometry)\n    returns geography\n    language sql\n    immutable strict parallel safe\n    as $$\n    select\n        g::geography\n$$;\n";
        assert_eq!(
            findings(Rule::FunctionBodyBound, &[(FILE, sql)])?,
            ["migrations/branch/0001_functions.sql:1:17"]
        );
        Ok(())
    }

    #[test]
    fn an_unqualified_relation_static_or_dynamic_is_refused() -> Result<(), FixtureError> {
        let sql = "create function count_entities ()\n    returns bigint\n    language plpgsql\n    as $$\ndeclare\n    counted bigint;\nbegin\n    select count(*) into counted from entities;\n    execute format('select count(*) from %I.entities', 'public') into counted;\n    return counted;\nend\n$$;\n";
        assert_eq!(
            findings(Rule::FunctionBodyBound, &[(FILE, sql)])?,
            ["migrations/branch/0001_functions.sql:1:17", "migrations/branch/0001_functions.sql:1:17"]
        );
        Ok(())
    }
}
