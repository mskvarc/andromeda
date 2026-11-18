//! What an entry point makes of one input, and the snapshot text that records
//! it.

use crate::parse_behaviour_tests::entry_point::EntryPoint;
use std::fmt::{self, Debug, Display, Write};

/// What an entry point makes of one input.
pub(super) enum Outcome {
    /// The input parses.
    Accepted {
        /// The `Debug` of the tree it parses to.
        tree: String,
        /// The tree written back as text, for an entry point whose tree has a
        /// written form.
        written_back: Option<WrittenBack>,
    },
    /// The input is refused.
    Refused {
        /// The `Debug` of the error.
        error: String,
        /// The `Display` of the error, the detail text a client is answered
        /// with.
        detail: String,
    },
}

/// An accepted tree written back as text, and what reading that text gives.
pub(super) struct WrittenBack {
    /// The text the tree is written back as.
    text: String,
    /// What reading the text again gives.
    reading: Reading,
}

/// What reading a written-back text gives, against the tree it was written
/// from.
enum Reading {
    /// The tree it was written from.
    SameTree,
    /// Another tree, as its `Debug`.
    OtherTree(String),
    /// A refusal, as the `Debug` of its error.
    Refused(String),
}

/// What parsing `text` with `parse` gives, for a tree with no written form.
pub(super) fn read<Tree, Error>(text: &str, parse: impl Fn(&str) -> Result<Tree, Error>) -> Outcome
where
    Tree: Debug,
    Error: Debug + Display,
{
    match parse(text) {
        Ok(tree) => Outcome::Accepted {
            tree: format!("{tree:?}"),
            written_back: None,
        },
        Err(error) => refused(&error),
    }
}

/// What parsing `text` with `parse` gives, the accepted tree written back with
/// `write` and read again.
pub(super) fn read_and_write_back<Tree, Error>(text: &str, parse: impl Fn(&str) -> Result<Tree, Error>, write: impl Fn(&Tree) -> String) -> Outcome
where
    Tree: Debug + PartialEq,
    Error: Debug + Display,
{
    match parse(text) {
        Ok(tree) => {
            let written = write(&tree);
            let reading = match parse(&written) {
                Ok(reread) if reread == tree => Reading::SameTree,
                Ok(reread) => Reading::OtherTree(format!("{reread:?}")),
                Err(error) => Reading::Refused(format!("{error:?}")),
            };
            Outcome::Accepted {
                tree: format!("{tree:?}"),
                written_back: Some(WrittenBack { text: written, reading }),
            }
        }
        Err(error) => refused(&error),
    }
}

/// The outcome of a refusal with `error`.
fn refused<Error: Debug + Display>(error: &Error) -> Outcome {
    Outcome::Refused {
        error: format!("{error:?}"),
        detail: error.to_string(),
    }
}

/// The snapshot text of every input of `entry_point`'s corpus: the input as
/// its `Debug`, then one indented line for each fact recorded about it.
pub(super) fn recorded(entry_point: EntryPoint) -> String {
    let mut text = String::new();
    for input in entry_point.corpus() {
        let outcome = entry_point.outcome(&input);
        write_record(&mut text, &input, outcome).expect("a string takes every write");
    }
    text
}

/// Writes the record of `input` and its `outcome` to `text`.
fn write_record(text: &mut String, input: &str, outcome: Outcome) -> fmt::Result {
    writeln!(text, "{input:?}")?;
    match outcome {
        Outcome::Accepted { tree, written_back } => {
            writeln!(text, "  accepted {tree}")?;
            match written_back {
                Some(WrittenBack {
                    text: written,
                    reading: Reading::SameTree,
                }) => writeln!(text, "  written {written:?}, read back to the same tree"),
                Some(WrittenBack {
                    text: written,
                    reading: Reading::OtherTree(tree),
                }) => {
                    writeln!(text, "  written {written:?}, read back to another tree {tree}")
                }
                Some(WrittenBack {
                    text: written,
                    reading: Reading::Refused(error),
                }) => {
                    writeln!(text, "  written {written:?}, refused when read back with {error}")
                }
                None => Ok(()),
            }
        }
        Outcome::Refused { error, detail } => {
            writeln!(text, "  refused {error}")?;
            writeln!(text, "  variant {}", variant_path(&error))?;
            writeln!(text, "  detail {detail:?}")
        }
    }
}

/// The variant path of an error's `Debug`: the run of identifiers at its
/// start that are each followed by `(`, joined by `/`, ending with the first
/// identifier not followed by `(`.
pub(super) fn variant_path(debug: &str) -> String {
    let mut segments = Vec::new();
    let mut rest = debug;
    while rest.starts_with(|character: char| character.is_ascii_alphabetic() || character == '_') {
        let end = rest
            .find(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
            .unwrap_or(rest.len());
        let (identifier, after) = rest.split_at(end);
        segments.push(identifier);
        let Some(inner) = after.strip_prefix('(') else { break };
        rest = inner;
    }
    segments.join("/")
}
