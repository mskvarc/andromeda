//! That a specification crate does not spell a storage dialect, read from a
//! crate's own sources.

use std::{fs, path::Path};

/// Fragments that only ever appear in a rendering, never in a grammar.
///
/// The casts and the PostGIS and JSONB function prefixes are the shape a value
/// takes in one store; `SELECT`, `= ANY(`, `IS NULL`, `AND` and `OR` are
/// statement composition; `<>` is a second spelling of an operator clause 4.9
/// writes `!=`, which is what one resource's enum returned while every other
/// renderer in the tree emitted the clause's own token.
const DIALECT: &[&str] = &[
    "::numeric",
    "::boolean",
    "::timestamptz",
    "::lquery",
    "::geography",
    "ST_",
    "jsonb_",
    "= ANY(",
    "SELECT ",
    "IS NULL",
    " AND ",
    " OR ",
    "<>",
];

/// Assert that no source file under `crate_root/src` writes a storage dialect.
///
/// The manifest guard reads dependency names, which is exactly what a dialect
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
pub fn assert_no_dialect(crate_root: &str) {
    let root = Path::new(crate_root).join("src");
    let sources = rust_sources(&root);
    assert!(!sources.is_empty(), "{} holds no source files to scan", root.display());

    for (path, source) in sources {
        let code = code_of(&source);
        for fragment in DIALECT {
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
/// with comment lines removed.
///
/// A doc comment may name the counterpart a backend renders — that is how two
/// readings of one clause stay comparable. A test may name one too, and for the
/// same reason the manifest guard admits a runtime under `[dev-dependencies]`:
/// a test asserts *about* a rendering without being one, and nothing it
/// contains reaches a caller. Cutting at the first `#[cfg(test)]` is also the
/// measure this project already uses for its file-length rule.
///
/// The kept lines are rejoined with newlines rather than concatenated: fusing
/// the end of one line to the start of the next invents fragments neither line
/// contains.
fn code_of(source: &str) -> String {
    let shipped = source.split_once("#[cfg(test)]").map_or(source, |(before, _)| before);
    shipped
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<&str>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use crate::dialect::{code_of, contains_fragment};

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
}
