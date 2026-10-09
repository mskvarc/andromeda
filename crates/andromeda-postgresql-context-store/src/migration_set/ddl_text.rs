//! The lexical reading of migration DDL: comments, statements, keywords and
//! parenthesised lists, matched whatever case a statement is written in.
//!
//! The reader of declared objects builds on these alone, so each rule about
//! how migration text is tokenised lives in one place.

/// `text` with `prefix` stripped from its start, ignoring ASCII case, or `None`
/// when `text` does not begin with it. SQL keywords are canonically lower case,
/// but the reader matches them whatever case a statement is written in.
pub(super) fn strip_prefix_ci<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix).then(|| &text[prefix.len()..])
}

/// Whether `text` begins with `prefix`, ignoring ASCII case.
pub(super) fn starts_with_ci(text: &str, prefix: &str) -> bool {
    text.get(..prefix.len()).is_some_and(|head| head.eq_ignore_ascii_case(prefix))
}

/// The byte offset of the first occurrence of `needle` in `text`, ignoring
/// ASCII case. `to_ascii_uppercase` preserves byte length, so the offset indexes
/// `text` as well as its upper-cased copy.
pub(super) fn find_ci(text: &str, needle: &str) -> Option<usize> {
    text.to_ascii_uppercase().find(&needle.to_ascii_uppercase())
}

/// Whether `text` contains `needle`, ignoring ASCII case.
pub(super) fn contains_ci(text: &str, needle: &str) -> bool {
    find_ci(text, needle).is_some()
}

/// The byte offset of the last occurrence of `needle` in `text`, ignoring ASCII
/// case. `to_ascii_uppercase` preserves byte length, so the offset indexes
/// `text` as well as its upper-cased copy.
pub(super) fn rfind_ci(text: &str, needle: &str) -> Option<usize> {
    text.to_ascii_uppercase().rfind(&needle.to_ascii_uppercase())
}

/// `sql` with every `--` comment removed.
pub(super) fn without_comments(sql: &str) -> String {
    sql.lines()
        .map(|line| line.split_once("--").map_or(line, |(code, _)| code))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The statements of `text`, split on the semicolons outside a dollar-quoted
/// function body, each trimmed and the empty ones dropped.
pub(super) fn statements(text: &str) -> Vec<&str> {
    let mut statements = Vec::new();
    let mut quoted = false;
    let mut start = 0;
    let mut rest = text.char_indices().peekable();
    while let Some((at, character)) = rest.next() {
        if character == '$' && rest.next_if(|(_, next)| *next == '$').is_some() {
            quoted = !quoted;
        } else if character == ';' && !quoted {
            statements.push(text[start..at].trim());
            start = at + 1;
        }
    }
    statements.push(text[start..].trim());
    statements.into_iter().filter(|statement| !statement.is_empty()).collect()
}

/// The byte offset of the parenthesis that closes the one `text` follows.
pub(super) fn closing_parenthesis(text: &str) -> usize {
    let mut depth = 0_usize;
    for (at, character) in text.char_indices() {
        if character == '(' {
            depth += 1;
        } else if character == ')' {
            if depth == 0 {
                return at;
            }
            depth -= 1;
        }
    }
    text.len()
}

/// `text` split on the commas that are not inside parentheses, each item
/// trimmed and the empty ones dropped.
pub(super) fn top_level_items(text: &str) -> Vec<&str> {
    let mut items = Vec::new();
    let mut depth = 0_usize;
    let mut start = 0;
    for (at, character) in text.char_indices() {
        if character == '(' {
            depth += 1;
        } else if character == ')' {
            depth = depth.saturating_sub(1);
        } else if character == ',' && depth == 0 {
            items.push(text[start..at].trim());
            start = at + 1;
        }
    }
    items.push(text[start..].trim());
    items.into_iter().filter(|item| !item.is_empty()).collect()
}

/// The names in the first parenthesised list of `text`.
pub(super) fn first_column_list(text: &str) -> Vec<String> {
    text.split_once('(')
        .and_then(|(_, rest)| rest.split_once(')'))
        .map(|(list, _)| list.split(',').map(|column| column.trim().to_owned()).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use crate::migration_set::ddl_text::statements;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_semicolon_inside_a_function_body_does_not_end_the_statement() {
        let sql = "create function f ()\n    returns trigger\n    language plpgsql\n    as $$\nbegin\n    return null;\nend\n$$;\ncreate table t (a bigint);";
        assert_eq!(
            statements(sql),
            [
                "create function f ()\n    returns trigger\n    language plpgsql\n    as $$\nbegin\n    return null;\nend\n$$",
                "create table t (a bigint)"
            ]
        );
    }
}
