//! The rejoining of a one-line parenthesised list onto the line that opens it.
//!
//! `pg_format` always opens a parenthesised list on a line of its own, even
//! when the list is one short line, and no option disables it
//! (`identity (\n    cache 1)`). A list whose opening parenthesis ends a line
//! and whose closing parenthesis ends the next one, with no parenthesis in
//! between, is put back on the opening line with the indentation dropped. A
//! list spanning several lines is left as formatted.

/// `sql` with every one-line parenthesised list rejoined onto its opening line.
#[must_use]
pub fn rejoin_one_line_lists(sql: &str) -> String {
    let mut rejoined = String::with_capacity(sql.len());
    let mut rest = sql;
    while let Some(opening) = rest.find("(\n") {
        let (before, list) = rest.split_at(opening);
        rejoined.push_str(before);
        let content = list[2..].trim_start_matches([' ', '\t']);
        match content.find(['\n', '(', ')']) {
            Some(closing) if content.as_bytes().get(closing) == Some(&b')') => {
                rejoined.push('(');
                rejoined.push_str(&content[..=closing]);
                rest = &content[closing + 1..];
            }
            Some(_) | None => {
                rejoined.push('(');
                rest = &list[1..];
            }
        }
    }
    rejoined.push_str(rest);
    rejoined
}

#[cfg(test)]
mod tests {
    use crate::list_rejoin::rejoin_one_line_lists;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_one_line_list_is_rejoined_onto_its_opening_line() {
        assert_eq!(
            rejoin_one_line_lists("    key bigint generated always as identity (\n        cache 1) primary key,\n"),
            "    key bigint generated always as identity (cache 1) primary key,\n"
        );
    }

    #[test]
    fn a_list_spanning_several_lines_is_left_as_formatted() {
        let sql = "create table t (\n    a integer,\n    b integer\n);\n";
        assert_eq!(rejoin_one_line_lists(sql), sql);
    }

    #[test]
    fn a_list_holding_a_nested_parenthesis_is_left_as_formatted() {
        let sql = "check (\n    length(a) < 4)\n";
        assert_eq!(rejoin_one_line_lists(sql), sql);
    }

    #[test]
    fn tabs_and_an_empty_list_are_rejoined() {
        assert_eq!(rejoin_one_line_lists("f (\n\t)\ng (\n \tx, y)\n"), "f ()\ng (x, y)\n");
    }

    #[test]
    fn every_list_of_the_text_is_rejoined() {
        assert_eq!(
            rejoin_one_line_lists("with (\n    fillfactor = 90) and (\n    cache 1)\n"),
            "with (fillfactor = 90) and (cache 1)\n"
        );
    }

    #[test]
    fn a_list_opened_inside_another_is_rejoined_alone() {
        assert_eq!(rejoin_one_line_lists("outer (\n    inner (\n        a)\n)\n"), "outer (\n    inner (a)\n)\n");
    }
}
