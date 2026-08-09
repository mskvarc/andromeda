//! That a specification crate does not spell SQL, read from its own sources.

use std::{fs, path::Path};

/// Fragments that only ever appear in a rendering, never in a grammar.
///
/// The casts and the PostGIS and JSONB function prefixes are the shape a value
/// takes in one store; `SELECT`, `= ANY(`, `IS NULL`, `AND` and `OR` are
/// statement composition; `<>` is a second spelling of an operator clause 4.9
/// writes `!=`, which is what one resource's enum returned while every other
/// renderer in the tree emitted the clause's own token.
const SQL_FRAGMENTS: &[&str] = &[
    "::numeric",
    "::boolean",
    "::timestamptz",
    "::geography",
    // Word-boundary rather than cast-only: `lquery` and `jsonb` name a type or a
    // function family a store spells in more than one syntactic position
    // (`::lquery`, a bare `lquery` operand; `::jsonb`, `jsonb_path_query`), and a
    // cast-only fragment let the others through.
    "lquery",
    "jsonb",
    "ST_",
    "= ANY(",
    "SELECT ",
    "IS NULL",
    " AND ",
    " OR ",
    "<>",
];

/// Assert that no source file under `crate_root/src` writes SQL fragments.
///
/// The dependency check reads dependency names, which is exactly what SQL
/// emitted as a string constant slips past: a specification crate can spell an
/// entire SQL predicate without declaring `sea-query`. Six items did — a
/// comparison operator, a POSIX regex operator, an `AND` composer, a JSONB
/// cast and two column mappings on a resource — and nothing in the workspace
/// could see them.
///
/// The directory is walked rather than a list of files being passed in: a list
/// goes stale the moment a file is added, and a guard that silently stops
/// covering new code reads as enforcement while providing none.
///
/// # Panics
///
/// Panics when a source file contains a dialect fragment outside a comment, or
/// when the source tree cannot be read.
pub fn assert_no_sql(crate_root: &str) {
    let root = Path::new(crate_root).join("src");
    let sources = rust_sources(&root);
    assert!(!sources.is_empty(), "{} holds no source files to scan", root.display());

    for (path, source) in sources {
        let code = code_of(&source);
        for fragment in SQL_FRAGMENTS {
            assert!(
                !contains_fragment(&code, fragment),
                "{path} emits '{fragment}'; how a value is spelled in a store belongs to that backend"
            );
        }
    }
}

/// Whether `code` contains `fragment` other than as the tail of a longer word.
///
/// A fragment that starts with an identifier character names something a store
/// spells, not a suffix of a Rust name: `ST_` is the PostGIS function prefix,
/// and matching it anywhere would also match the end of `CARGO_MANIFEST_DIR`.
/// Fragments that open with punctuation — a cast, an operator, a separator —
/// carry their own boundary and are matched as written.
fn contains_fragment(code: &str, fragment: &str) -> bool {
    let bounded = fragment.starts_with(|first: char| first.is_alphanumeric());
    code.match_indices(fragment).any(|(offset, _)| {
        !bounded
            || code[..offset]
                .chars()
                .next_back()
                .is_none_or(|preceding| !preceding.is_alphanumeric() && preceding != '_')
    })
}

/// Every `.rs` file under a directory, paired with its path relative to it.
fn rust_sources(root: &Path) -> Vec<(String, String)> {
    let mut sources = Vec::new();
    let mut directories = vec![root.to_path_buf()];

    while let Some(directory) = directories.pop() {
        let entries = fs::read_dir(&directory).unwrap_or_else(|error| unreachable!("{} is readable: {error}", directory.display()));
        for entry in entries {
            let path = entry
                .unwrap_or_else(|error| unreachable!("an entry of {} is readable: {error}", directory.display()))
                .path();
            if path.is_dir() {
                directories.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                let source = fs::read_to_string(&path).unwrap_or_else(|error| unreachable!("{} is readable: {error}", path.display()));
                let relative = path.strip_prefix(root).unwrap_or(&path).display().to_string();
                sources.push((relative, source));
            }
        }
    }

    sources.sort();
    sources
}

/// The code a source file ships: everything before its first `#[cfg(test)]`,
/// with comments removed.
///
/// A doc comment may name the counterpart a backend renders — that is how two
/// readings of one clause stay comparable. A test may name one too, and for the
/// same reason the manifest guard admits a runtime under `[dev-dependencies]`:
/// a test asserts *about* a rendering without being one, and nothing it
/// contains reaches a caller. Cutting at the first `#[cfg(test)]` is also the
/// measure this project already uses for its file-length rule.
///
/// Comments are removed *before* the `#[cfg(test)]` split, not after: a comment
/// that merely mentions `#[cfg(test)]` would otherwise truncate the scan there
/// and hide every real fragment below it.
fn code_of(source: &str) -> String {
    let without_comments = strip_comments(source);
    match without_comments.split_once("#[cfg(test)]") {
        Some((before, _)) => before.to_string(),
        None => without_comments,
    }
}

/// Remove `//` line comments and `/* … */` block comments — the latter nested
/// to any depth — from Rust source, leaving string and character literals
/// intact.
///
/// A line-based filter cannot do this: it misses a block comment that names a
/// backend fragment (a false positive), and it cannot tell a `/*` inside a real
/// string from one that opens a comment. String and character literals are
/// therefore copied verbatim, so a dialect fragment written as a genuine value
/// (`"::jsonb"`) is still scanned while a `//` or `/*` inside a literal never
/// opens a comment. Raw strings (`r"…"`, `r#"…"#`, byte-raw `br"…"`) are honoured
/// so an embedded `"` does not end the literal early. Each block comment
/// collapses to a single space so the tokens on either side are not fused into a
/// fragment neither of them contains.
fn strip_comments(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::with_capacity(source.len());
    let mut i = 0;
    while i < chars.len() {
        let rest = &chars[i..];
        if rest.starts_with(&['/', '/']) {
            i += 2;
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if rest.starts_with(&['/', '*']) {
            i = skip_block_comment(&chars, i + 2);
            // A separator, so the tokens on either side are not fused.
            out.push(' ');
        } else if chars[i] == '"' {
            i = copy_string(&chars, i, &mut out);
        } else if is_raw_string_start(&chars, i) {
            i = copy_raw_string(&chars, i, &mut out);
        } else if chars[i] == '\'' {
            i = copy_char_or_lifetime(&chars, i, &mut out);
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// The index just past the `*/` closing a block comment opened before `start`,
/// counting nested `/* … */` pairs so the whole comment is consumed.
fn skip_block_comment(chars: &[char], start: usize) -> usize {
    let mut depth = 1usize;
    let mut i = start;
    while i < chars.len() && depth > 0 {
        if chars[i..].starts_with(&['/', '*']) {
            depth += 1;
            i += 2;
        } else if chars[i..].starts_with(&['*', '/']) {
            depth -= 1;
            i += 2;
        } else {
            i += 1;
        }
    }
    i
}

/// Copy the double-quoted string literal at `i` verbatim into `out`, and return
/// the index just past its closing quote. A backslash escapes the next
/// character, so an escaped quote does not end the literal.
fn copy_string(chars: &[char], i: usize, out: &mut String) -> usize {
    out.push(chars[i]);
    let mut j = i + 1;
    while j < chars.len() {
        let current = chars[j];
        out.push(current);
        if current == '\\' && j + 1 < chars.len() {
            out.push(chars[j + 1]);
            j += 2;
            continue;
        }
        j += 1;
        if current == '"' {
            break;
        }
    }
    j
}

/// Whether a raw string literal — `r"…"`, `r#"…"#`, or a byte-raw `br"…"` — opens
/// at `i`, rather than an identifier that merely begins with `r` or `b`.
fn is_raw_string_start(chars: &[char], i: usize) -> bool {
    if i > 0 && is_ident_char(chars[i - 1]) {
        return false;
    }
    let mut j = i;
    if chars.get(j) == Some(&'b') {
        j += 1;
    }
    if chars.get(j) != Some(&'r') {
        return false;
    }
    j += 1;
    while chars.get(j) == Some(&'#') {
        j += 1;
    }
    chars.get(j) == Some(&'"')
}

/// Copy the raw string literal at `i` verbatim into `out`, and return the index
/// just past the closing quote and its matching `#`s. Nothing inside a raw
/// string escapes, so only a `"` followed by exactly the opening number of `#`s
/// closes it.
fn copy_raw_string(chars: &[char], i: usize, out: &mut String) -> usize {
    let mut j = i;
    if chars[j] == 'b' {
        out.push(chars[j]);
        j += 1;
    }
    out.push(chars[j]); // the `r`
    j += 1;
    let mut hashes = 0usize;
    while chars.get(j) == Some(&'#') {
        out.push('#');
        hashes += 1;
        j += 1;
    }
    out.push(chars[j]); // the opening quote
    j += 1;
    while j < chars.len() {
        if chars[j] == '"' && closing_hashes_match(chars, j + 1, hashes) {
            out.push('"');
            for _ in 0..hashes {
                out.push('#');
            }
            return j + 1 + hashes;
        }
        out.push(chars[j]);
        j += 1;
    }
    j
}

/// Whether `hashes` `#` characters begin at `start`, the tail a raw string's
/// closing quote needs to match its opening `#`s.
fn closing_hashes_match(chars: &[char], start: usize, hashes: usize) -> bool {
    (0..hashes).all(|offset| chars.get(start + offset) == Some(&'#'))
}

/// Copy the character literal at `i` verbatim, or emit the lone `'` of a
/// lifetime, and return the next index to scan.
///
/// The distinction matters only so a character literal holding a quote (`'"'`)
/// does not open a phantom string; a lifetime's `'` is emitted as an ordinary
/// character.
fn copy_char_or_lifetime(chars: &[char], i: usize, out: &mut String) -> usize {
    if chars.get(i + 1) == Some(&'\\') {
        out.push(chars[i]);
        let mut j = i + 1;
        while j < chars.len() {
            out.push(chars[j]);
            if chars[j] == '\'' && chars[j - 1] != '\\' {
                return j + 1;
            }
            j += 1;
        }
        return j;
    }
    if chars.get(i + 2) == Some(&'\'') {
        out.push(chars[i]);
        out.push(chars[i + 1]);
        out.push(chars[i + 2]);
        return i + 3;
    }
    out.push(chars[i]);
    i + 1
}

/// Whether a character can appear inside a Rust identifier.
fn is_ident_char(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

#[cfg(test)]
mod tests {
    use crate::sql::{code_of, contains_fragment};

    #[test]
    fn a_fragment_that_opens_with_an_identifier_character_is_matched_at_a_word_boundary() {
        // `ST_` names the PostGIS prefix, and every crate's guard passes
        // `CARGO_MANIFEST_DIR`, which ends in the same three characters.
        assert!(!contains_fragment("env!(\"CARGO_MANIFEST_DIR\")", "ST_"));
        assert!(contains_fragment("format!(\"ST_Within({a}, {b})\")", "ST_"));
    }

    #[test]
    fn a_fragment_that_opens_with_punctuation_is_matched_as_written() {
        // ` AND ` is preceded by whatever the predicate ended with, so a
        // boundary test would reject every real occurrence of it.
        assert!(contains_fragment("format!(\"({a} AND {b})\")", " AND "));
        assert!(contains_fragment("let cast = \"::numeric\";", "::numeric"));
    }

    #[test]
    fn a_comment_may_name_the_counterpart_a_backend_renders() {
        let source = "// `ST_Within(subject, reference)`\nGeoRel::Within => subject.is_within(reference),";
        assert!(!code_of(source).contains("ST_"));
    }

    #[test]
    fn a_fragment_in_code_survives_the_comment_stripping() {
        let source = "let cast = \"::numeric\";";
        assert!(code_of(source).contains("::numeric"));
    }

    #[test]
    fn a_test_module_may_name_what_it_asserts_about() {
        let source = "pub fn parse() {}\n\n#[cfg(test)]\nmod tests {\n    let message = \"expected And as second OR term\";\n}";
        assert!(!code_of(source).contains(" OR "));
    }

    #[test]
    fn stripping_a_comment_does_not_fuse_the_lines_around_it() {
        // Concatenating the kept lines used to splice `SELECT` onto a following
        // space and invent a fragment neither line contains.
        let source = "let keyword = \"SELECT\";\n    let subject = value;";
        assert!(!code_of(source).contains("SELECT "));
    }

    #[test]
    fn a_jsonb_cast_and_a_bare_lquery_operand_are_both_caught() {
        // The cast-only fragments used to let `::jsonb` and a bare `lquery`
        // operand through while catching only `jsonb_...` and `::lquery`.
        assert!(contains_fragment("let sql = \"value::jsonb\";", "jsonb"));
        assert!(contains_fragment("let sql = \"jsonb_path_query(x)\";", "jsonb"));
        assert!(contains_fragment("let sql = \"path ~ 'a.b'::lquery\";", "lquery"));
        assert!(contains_fragment("let sql = \"lquery(col)\";", "lquery"));
    }

    #[test]
    fn a_comment_that_mentions_cfg_test_does_not_truncate_the_scan() {
        // The comment names `#[cfg(test)]`; the real dialer below it must still
        // be scanned, which the split-before-strip order used to skip.
        let source = "// this is not the #[cfg(test)] module\nlet cast = \"value::jsonb\";";
        assert!(code_of(source).contains("::jsonb"));
    }

    #[test]
    fn a_block_comment_naming_a_backend_fragment_is_ignored() {
        // A line-based filter kept a `/* … */` block whole, so a fragment named
        // only in one would have failed the guard.
        let source = "let subject = value; /* ST_Within(subject, reference) */";
        assert!(!code_of(source).contains("ST_"));
    }

    #[test]
    fn a_fragment_inside_a_string_literal_is_still_caught() {
        // A real rendering written as a string value must survive the stripping.
        let code = code_of("let sql = \"value::jsonb\";");
        assert!(contains_fragment(&code, "jsonb"));
    }

    #[test]
    fn a_block_comment_opener_inside_a_string_does_not_swallow_the_code_after_it() {
        // `"/*"` is a string, not the start of a comment, so the dialect after
        // it is still scanned.
        let source = "let opener = \"/*\"; let cast = \"::numeric\";";
        assert!(contains_fragment(&code_of(source), "::numeric"));
    }

    #[test]
    fn a_nested_block_comment_is_stripped_to_its_outermost_close() {
        // The depth counter must not stop at the first `*/`, or the code after a
        // nested comment would be dropped and its dialect never seen.
        let source = "let a = 1; /* outer /* ST_Within */ still comment */ let cast = \"::timestamptz\";";
        let code = code_of(source);
        assert!(!code.contains("ST_"));
        assert!(contains_fragment(&code, "::timestamptz"));
    }
}
