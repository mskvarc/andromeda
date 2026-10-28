//! The words `pg_format` is taught beside the ones it knows, and which called
//! functions it knows.
//!
//! `pg_format` lays a call of a function it knows out as `name(`, and one it
//! does not as `name (`, so the canonical layout of a migration depends on its
//! vocabulary. Which names it knows is asked of the pinned `pg_format` itself,
//! in one run over a statement calling every name once, so the answer follows
//! the release rather than a copied list.

use crate::pg_format::{PgFormat, PgFormatError};
use pg_query::protobuf::Token;
use std::{collections::BTreeSet, fs, io, ops::Range, path::Path, sync::Arc};

/// A repository-root file of words `pg_format` is taught.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, strum::Display, strum::AsRefStr, strum::EnumIter)]
pub enum VocabularyFile {
    /// The functions it lays out as calls.
    #[strum(serialize = ".pg_format_functions")]
    Functions,
    /// The keywords it cases like its own.
    #[strum(serialize = ".pg_format_keywords")]
    Keywords,
}

/// One word of a vocabulary file, and where it is written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VocabularyEntry {
    /// The word, as written.
    word: String,
    /// The bytes of the file it covers.
    range: Range<usize>,
}

impl VocabularyEntry {
    /// The word, as written.
    #[must_use]
    pub fn word(&self) -> &str {
        &self.word
    }

    /// The bytes of the file it covers.
    #[must_use]
    pub fn range(&self) -> Range<usize> {
        self.range.clone()
    }
}

/// One vocabulary file with its text and its words.
#[derive(Debug, Clone)]
pub struct VocabularyList {
    /// The file.
    file: VocabularyFile,
    /// Its text.
    text: Arc<str>,
    /// Its words, in the order written.
    entries: Vec<VocabularyEntry>,
}

impl VocabularyList {
    /// The list `text`, the content of `file`, holds: one word per
    /// whitespace-separated run.
    #[must_use]
    pub fn new(file: VocabularyFile, text: Arc<str>) -> VocabularyList {
        let mut entries = Vec::new();
        let mut start = None;
        for (offset, character) in text.char_indices().chain([(text.len(), ' ')]) {
            match (character.is_whitespace(), start) {
                (true, Some(begin)) => {
                    entries.push(VocabularyEntry {
                        word: text[begin..offset].to_owned(),
                        range: begin..offset,
                    });
                    start = None;
                }
                (false, None) => start = Some(offset),
                (true, None) | (false, Some(_)) => {}
            }
        }
        VocabularyList { file, text, entries }
    }

    /// The file.
    #[must_use]
    pub const fn file(&self) -> VocabularyFile {
        self.file
    }

    /// Its text.
    #[must_use]
    pub const fn text(&self) -> &Arc<str> {
        &self.text
    }

    /// Its words, in the order written.
    #[must_use]
    pub fn entries(&self) -> &[VocabularyEntry] {
        &self.entries
    }
}

/// Both vocabulary files, and the called functions `pg_format` knows.
#[derive(Debug, Clone)]
pub struct FormatterVocabulary {
    /// `.pg_format_functions`.
    functions: VocabularyList,
    /// `.pg_format_keywords`.
    keywords: VocabularyList,
    /// The called names, lowercase, `pg_format` lays out as calls.
    known_functions: BTreeSet<String>,
}

impl FormatterVocabulary {
    /// The vocabulary of `functions` and `keywords`, with `known_functions`
    /// the lowercase names `pg_format` lays out as calls.
    #[must_use]
    pub const fn new(functions: VocabularyList, keywords: VocabularyList, known_functions: BTreeSet<String>) -> FormatterVocabulary {
        FormatterVocabulary {
            functions,
            keywords,
            known_functions,
        }
    }

    /// Read both files at `root` and ask `pg_format` which of `called`, the
    /// names the migrations call, it lays out as calls.
    ///
    /// # Errors
    ///
    /// Returns [`VocabularyError`] when a file cannot be read or `pg_format`
    /// fails.
    pub fn probe<'name>(root: &Path, pg_format: &PgFormat, called: impl IntoIterator<Item = &'name str>) -> Result<FormatterVocabulary, VocabularyError> {
        let read = |file: VocabularyFile| {
            fs::read_to_string(root.join(file.as_ref()))
                .map(|text| VocabularyList::new(file, Arc::from(text)))
                .map_err(|source| VocabularyError::Read { file, source })
        };
        let names = called
            .into_iter()
            .map(str::to_ascii_lowercase)
            .filter(|name| is_plain_identifier(name))
            .collect::<BTreeSet<_>>();
        Ok(FormatterVocabulary {
            functions: read(VocabularyFile::Functions)?,
            keywords: read(VocabularyFile::Keywords)?,
            known_functions: known_calls(pg_format, &names)?,
        })
    }

    /// `.pg_format_functions`.
    #[must_use]
    pub const fn functions(&self) -> &VocabularyList {
        &self.functions
    }

    /// `.pg_format_keywords`.
    #[must_use]
    pub const fn keywords(&self) -> &VocabularyList {
        &self.keywords
    }

    /// Whether `pg_format` lays a call of `name` out as a call.
    #[must_use]
    pub fn knows(&self, name: &str) -> bool {
        self.known_functions.contains(&name.to_ascii_lowercase())
    }
}

/// Whether `name` needs no quoting: a lowercase letter or underscore, then
/// lowercase letters, digits and underscores.
fn is_plain_identifier(name: &str) -> bool {
    name.bytes().next().is_some_and(|first| first.is_ascii_lowercase() || first == b'_')
        && name.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

/// The names of `names` that `pg_format` writes directly before their
/// parenthesis when each is called once in one statement.
fn known_calls(pg_format: &PgFormat, names: &BTreeSet<String>) -> Result<BTreeSet<String>, VocabularyError> {
    if names.is_empty() {
        return Ok(BTreeSet::new());
    }
    let probe = format!("select {};\n", names.iter().map(|name| format!("{name}(1)")).collect::<Vec<_>>().join(", "));
    let formatted = pg_format.format(&probe)?;
    let tokens = pg_query::scan(&formatted).map_err(VocabularyError::Unscanned)?.tokens;
    Ok(tokens
        .windows(2)
        .filter(|pair| Token::try_from(pair[1].token).ok() == Some(Token::Ascii40) && pair[0].end == pair[1].start)
        .filter_map(|pair| {
            let start = usize::try_from(pair[0].start).ok()?;
            let end = usize::try_from(pair[0].end).ok()?;
            formatted.get(start..end).map(str::to_ascii_lowercase)
        })
        .filter(|name| names.contains(name))
        .collect())
}

/// Why the vocabulary could not be read or probed.
#[derive(Debug, thiserror::Error)]
pub enum VocabularyError {
    /// A vocabulary file could not be read.
    #[error("{file} could not be read: {source}")]
    Read {
        /// The file concerned.
        file: VocabularyFile,
        /// What reading it failed with.
        source: io::Error,
    },
    /// `pg_format` could not lay the probe out.
    #[error(transparent)]
    PgFormat(#[from] PgFormatError),
    /// What `pg_format` returned could not be scanned.
    #[error("the probe pg_format returned could not be scanned: {0}")]
    Unscanned(pg_query::Error),
}

#[cfg(test)]
mod tests {
    use crate::formatter_vocabulary::{VocabularyFile, VocabularyList};
    use pretty_assertions::assert_eq;
    use std::sync::Arc;

    #[test]
    fn every_whitespace_separated_word_is_an_entry_with_its_place() {
        let list = VocabularyList::new(VocabularyFile::Functions, Arc::from("by_range\n  ST_IsValid\n"));
        let entries: Vec<(&str, usize)> = list.entries().iter().map(|entry| (entry.word(), entry.range().start)).collect();
        assert_eq!(entries, [("by_range", 0), ("ST_IsValid", 11)]);
    }
}
