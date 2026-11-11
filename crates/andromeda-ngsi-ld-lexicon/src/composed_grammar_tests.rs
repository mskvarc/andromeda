//! Every top-level grammar of the crate is checked in a second time, followed by the shared Unicode rules, because the grammar derive reads a single file resolved under the crate's own directory. A composed copy that differs from that composition, or one no grammar composes, fails here.

use andromeda_architecture_checks::generated_artifact::assert_checked_in;
use std::{collections::BTreeSet, fs, path::Path};

/// The repository root, relative to which the checked-in copies lie.
const WORKSPACE_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

/// The crate's grammars, relative to the repository root.
const GRAMMAR_DIRECTORY: &str = "crates/andromeda-ngsi-ld-lexicon/grammar";

/// The composed copies, relative to the grammar directory.
const COMPOSED_DIRECTORY: &str = ".generated";

/// The rules every top-level grammar is followed by. A grammar whose name
/// starts with `_` is such an include and is not composed on its own.
const SHARED_RULES: &str = "_unicode.abnf";

/// The names of the ABNF files directly in `directory`.
fn abnf_names(directory: &Path) -> BTreeSet<String> {
    fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("{} cannot be listed: {error}", directory.display()))
        .map(|entry| entry.unwrap_or_else(|error| panic!("{} cannot be listed: {error}", directory.display())).path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "abnf"))
        .filter_map(|path| path.file_name().and_then(|name| name.to_str()).map(str::to_owned))
        .collect()
}

/// The grammars a type derives from, each composed on its own.
fn top_level_grammars(grammars: &Path) -> BTreeSet<String> {
    abnf_names(grammars).into_iter().filter(|name| !name.starts_with('_')).collect()
}

/// `top` followed by `shared`, on a line of its own.
fn composition(top: &str, shared: &str) -> String {
    let separator = if top.ends_with('\n') { "" } else { "\n" };
    [top, separator, shared].concat()
}

/// The text of the file at `path`.
fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|error| panic!("{} cannot be read: {error}", path.display()))
}

#[test]
fn every_composed_grammar_is_its_grammar_followed_by_the_shared_rules() {
    let grammars = Path::new(WORKSPACE_ROOT).join(GRAMMAR_DIRECTORY);
    let shared = read(&grammars.join(SHARED_RULES));
    for name in top_level_grammars(&grammars) {
        let top = read(&grammars.join(&name));
        assert_checked_in(
            WORKSPACE_ROOT,
            &format!("{GRAMMAR_DIRECTORY}/{COMPOSED_DIRECTORY}/{name}"),
            &composition(&top, &shared),
        );
    }
}

#[test]
fn every_composed_grammar_is_composed_from_a_grammar() {
    let grammars = Path::new(WORKSPACE_ROOT).join(GRAMMAR_DIRECTORY);
    let sources = top_level_grammars(&grammars);
    let orphans: Vec<String> = abnf_names(&grammars.join(COMPOSED_DIRECTORY)).difference(&sources).cloned().collect();
    assert!(orphans.is_empty(), "no grammar composes {orphans:?}; delete them");
}

#[test]
fn a_grammar_without_a_final_newline_is_separated_from_the_shared_rules() {
    pretty_assertions::assert_eq!(composition("a = b", "c = d\n"), "a = b\nc = d\n");
    pretty_assertions::assert_eq!(composition("a = b\n", "c = d\n"), "a = b\nc = d\n");
}
