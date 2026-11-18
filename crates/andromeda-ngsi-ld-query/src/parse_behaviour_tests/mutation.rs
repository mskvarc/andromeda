//! One-character mutations of an input, drawn at random.

use proptest::{
    prelude::{Strategy, any},
    sample::{Index, select},
};
use strum::IntoEnumIterator;

/// The characters a mutation inserts or puts in place of another: the
/// punctuation of every language, a space and a tab, letters and digits at
/// either end of their ranges, and one character outside ASCII.
const MUTATION_CHARACTERS: [char; 32] = [
    ';', '|', '(', ')', '[', ']', '{', '}', ',', '.', ':', '=', '!', '~', '<', '>', '"', '*', '\\', '-', '+', '#', '/', '@', '%', ' ', '\t', 'a', 'Z', '0',
    '9', 'é',
];

/// The kind of a one-character mutation.
#[derive(Clone, Copy, Debug, strum::EnumIter)]
pub(super) enum MutationKind {
    /// Removes a character.
    Delete,
    /// Puts a character before another, or after the last one.
    Insert,
    /// Puts a character in place of another.
    Replace,
}

impl MutationKind {
    /// `text` with this change made at the character position `position`,
    /// putting in `character`. An insertion's position may be one past the
    /// last character.
    pub(super) fn applied_at(self, text: &str, position: usize, character: char) -> String {
        let mut characters: Vec<char> = text.chars().collect();
        match self {
            MutationKind::Delete => {
                characters.remove(position);
            }
            MutationKind::Insert => characters.insert(position, character),
            MutationKind::Replace => characters[position] = character,
        }
        characters.into_iter().collect()
    }
}

/// One drawn mutation.
#[derive(Clone, Debug)]
pub(super) struct Mutation {
    /// What the mutation does.
    pub(super) kind: MutationKind,
    /// Where it applies, drawn uniformly over the positions of whatever text
    /// it applies to.
    pub(super) at: Index,
    /// The character it inserts or puts in place of another.
    pub(super) character: char,
}

impl Mutation {
    /// `text` with the mutation applied. An empty text has no character to
    /// delete or replace, so every mutation of it inserts.
    pub(super) fn applied_to(&self, text: &str) -> String {
        let count = text.chars().count();
        match self.kind {
            MutationKind::Delete | MutationKind::Replace if count == 0 => MutationKind::Insert.applied_at(text, 0, self.character),
            MutationKind::Delete | MutationKind::Replace => self.kind.applied_at(text, self.at.index(count), self.character),
            MutationKind::Insert => self.kind.applied_at(text, self.at.index(count + 1), self.character),
        }
    }
}

/// A mutation of any kind, at any position, with any of the
/// [`MUTATION_CHARACTERS`].
pub(super) fn mutation() -> impl Strategy<Value = Mutation> {
    (
        select(MutationKind::iter().collect::<Vec<_>>()),
        any::<Index>(),
        select(&MUTATION_CHARACTERS[..]),
    )
        .prop_map(|(kind, at, character)| Mutation { kind, at, character })
}
