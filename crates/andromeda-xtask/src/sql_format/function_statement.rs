//! `CREATE FUNCTION` statements held out of `pg_format`.
//!
//! `pg_format` reformats the body of a `plpgsql` function as if it were plain
//! SQL, breaking its statements across lines and losing their indentation; it
//! unwraps a body written as a string literal the same way; and a
//! `language plpgsql` clause can shift the indentation of comments earlier in
//! the file. Each function statement is therefore replaced by a placeholder
//! statement selecting a string literal, which `pg_format` lays out like any
//! other statement and leaves the literal of, and put back verbatim afterwards:
//! a function keeps exactly the layout it is written in.

use regex::{Captures, Regex};

/// A whole `CREATE [OR REPLACE] FUNCTION` statement, from the start of its
/// line to the `$$;` that closes its dollar-quoted body.
const STATEMENT: &str = r"(?ims)^create (?:or replace )?function\b.*?\$\$.*?\$\$;";

/// The statement standing in for the function statement at the captured
/// position, however `pg_format` breaks its lines.
const PLACEHOLDER: &str = r"(?i)select\s+'__function_statement_(\d+)__';";

/// The two patterns a shield and its restoration match.
pub(crate) struct FunctionShield {
    /// A function statement.
    statement: Regex,
    /// A placeholder [`FunctionShield::shield`] left.
    placeholder: Regex,
}

/// Migration text whose function statements stand replaced by placeholders.
pub(crate) struct ShieldedSql<'a> {
    /// The text `pg_format` is given.
    pub(crate) text: String,
    /// The statements, each at the position its placeholder names.
    statements: Vec<&'a str>,
}

impl FunctionShield {
    /// Compile the two patterns.
    ///
    /// # Errors
    ///
    /// Fails if either pattern does not compile.
    pub(crate) fn new() -> Result<FunctionShield, regex::Error> {
        Ok(FunctionShield {
            statement: Regex::new(STATEMENT)?,
            placeholder: Regex::new(PLACEHOLDER)?,
        })
    }

    /// `sql` with every function statement replaced by its placeholder.
    pub(crate) fn shield<'a>(&self, sql: &'a str) -> ShieldedSql<'a> {
        let statements: Vec<&'a str> = self.statement.find_iter(sql).map(|found| found.as_str()).collect();
        let mut position = 0_usize;
        let text = self
            .statement
            .replace_all(sql, |_: &Captures<'_>| {
                let placeholder = format!("select '__function_statement_{position}__';");
                position += 1;
                placeholder
            })
            .into_owned();
        ShieldedSql { text, statements }
    }

    /// `formatted` with every placeholder of `shielded` replaced by its
    /// statement, or `None` when the placeholders are not exactly those
    /// `shielded` left, in order.
    pub(crate) fn restore(&self, shielded: &ShieldedSql<'_>, formatted: &str) -> Option<String> {
        let positions: Vec<usize> = self
            .placeholder
            .captures_iter(formatted)
            .map(|captures| captures[1].parse::<usize>().ok())
            .collect::<Option<_>>()?;
        if !positions.iter().copied().eq(0..shielded.statements.len()) {
            return None;
        }
        let mut statements = shielded.statements.iter();
        Some(
            self.placeholder
                .replace_all(formatted, |_: &Captures<'_>| statements.next().copied().unwrap_or_default())
                .into_owned(),
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::sql_format::function_statement::FunctionShield;

    fn shield() -> FunctionShield {
        FunctionShield::new().expect("the shield patterns compile")
    }

    const FUNCTION: &str = "create function f ()\n    returns trigger\n    language plpgsql\n    as $$\nbegin\n    return null;\nend\n$$;\n";

    #[test]
    fn a_function_statement_is_replaced_by_a_placeholder_statement() {
        let shield = shield();
        let sql = format!("{FUNCTION}\ncreate trigger t after insert on x for each statement execute function f ();\n");
        assert_eq!(
            shield.shield(&sql).text,
            "select '__function_statement_0__';\n\ncreate trigger t after insert on x for each statement execute function f ();\n"
        );
    }

    #[test]
    fn a_create_or_replace_statement_is_held_out_too() {
        let shield = shield();
        let sql = "create or replace function g (v integer)\n    returns integer\n    language sql\n    as $$\n    select\n        v\n$$;\n";
        assert_eq!(shield.shield(sql).text, "select '__function_statement_0__';\n");
    }

    #[test]
    fn every_statement_comes_back_verbatim_in_its_own_place() {
        let shield = shield();
        let sql = format!(
            "{FUNCTION}\ncreate table t (\n    a bigint\n);\n\n{}",
            FUNCTION.replace("return null;", "return new;")
        );
        let shielded = shield.shield(&sql);
        assert_eq!(shield.restore(&shielded, &shielded.text).as_deref(), Some(sql.as_str()));
    }

    #[test]
    fn a_body_holding_a_replacement_reference_is_restored_literally() {
        let shield = shield();
        let sql = "create function f ()\n    returns trigger\n    language plpgsql\n    as $$\nbegin\n    execute 'select $1' using x;\nend\n$$;\n";
        let shielded = shield.shield(sql);
        assert_eq!(shield.restore(&shielded, &shielded.text).as_deref(), Some(sql));
    }

    #[test]
    fn a_placeholder_broken_across_lines_is_restored() {
        let shield = shield();
        let shielded = shield.shield(FUNCTION);
        assert_eq!(
            shield.restore(&shielded, "select\n    '__function_statement_0__';\n").as_deref(),
            Some(FUNCTION)
        );
    }

    #[test]
    fn text_without_a_function_is_unchanged() {
        let shield = shield();
        let sql = "create table t (\n    a bigint\n);\n";
        let shielded = shield.shield(sql);
        assert_eq!(shielded.text, sql);
        assert_eq!(shield.restore(&shielded, sql).as_deref(), Some(sql));
    }

    #[test]
    fn a_lost_or_reordered_placeholder_is_refused() {
        let shield = shield();
        let sql = format!("{FUNCTION}{FUNCTION}");
        let shielded = shield.shield(&sql);
        assert_eq!(shield.restore(&shielded, "select\n    '__function_statement_0__';\n"), None);
        assert_eq!(
            shield.restore(&shielded, "select\n    '__function_statement_1__';\nselect\n    '__function_statement_0__';\n"),
            None
        );
    }
}
