//! The words `pg_format` is taught beside the ones it knows.
//!
//! `pg_format` cases the keywords it knows, so an extension's keyword it does
//! not know keeps the case it was typed in until `.pg_format_keywords` lists
//! it. A call is laid out from the parse, not from the formatter's dictionary
//! of functions, so no function is taught.

use std::{fs, io, ops::Range, path::Path, sync::Arc};

/// A repository-root file of words `pg_format` is taught.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, strum::Display, strum::AsRefStr, strum::EnumIter)]
pub enum VocabularyFile {
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

/// The words `pg_format` is taught beside its own.
#[derive(Debug, Clone)]
pub struct FormatterVocabulary {
    /// `.pg_format_keywords`.
    keywords: VocabularyList,
}

impl FormatterVocabulary {
    /// The vocabulary of `keywords`.
    #[must_use]
    pub const fn new(keywords: VocabularyList) -> FormatterVocabulary {
        FormatterVocabulary { keywords }
    }

    /// Read the vocabulary files at `root`.
    ///
    /// # Errors
    ///
    /// Returns [`VocabularyError`] when a file cannot be read.
    pub fn read(root: &Path) -> Result<FormatterVocabulary, VocabularyError> {
        let file = VocabularyFile::Keywords;
        let text = fs::read_to_string(root.join(file.as_ref())).map_err(|source| VocabularyError::Read { file, source })?;
        Ok(FormatterVocabulary {
            keywords: VocabularyList::new(file, Arc::from(text)),
        })
    }

    /// `.pg_format_keywords`.
    #[must_use]
    pub const fn keywords(&self) -> &VocabularyList {
        &self.keywords
    }
}

/// Why the vocabulary could not be read.
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
}

#[cfg(test)]
mod tests {
    use crate::formatter_vocabulary::{VocabularyFile, VocabularyList};
    use pretty_assertions::assert_eq;
    use std::sync::Arc;

    #[test]
    fn every_whitespace_separated_word_is_an_entry_with_its_place() {
        let list = VocabularyList::new(VocabularyFile::Keywords, Arc::from("BRIN\n  STATISTICS\n"));
        let entries: Vec<(&str, usize)> = list.entries().iter().map(|entry| (entry.word(), entry.range().start)).collect();
        assert_eq!(entries, [("BRIN", 0), ("STATISTICS", 7)]);
    }
}
