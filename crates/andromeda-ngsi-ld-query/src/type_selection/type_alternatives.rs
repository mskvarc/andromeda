//! The disjunction of conjunctions of entity types a type selection holds.

use crate::name_stage::NameStage;
use std::fmt::{self, Display, Formatter, Write};

/// The separator rendered between alternatives: `,` is OR (clause 4.17).
const ALTERNATIVE_SEPARATOR: char = ',';
/// The separator rendered between the types of one alternative: `;` is AND.
const REQUIREMENT_SEPARATOR: char = ';';

/// The clause 4.17 selection of entity types other than the wildcard: a
/// disjunction (`,` or `|`) of alternatives, each one entity type or a
/// parenthesised conjunction (`;`) of them.
///
/// Clause 4.17 nests no group inside another, so the tree is already in
/// disjunctive normal form and holds nothing deeper. Both levels are non-empty
/// by construction.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TypeAlternatives<S: NameStage>(
    /// The alternatives; at least one, each a non-empty conjunction of types.
    Vec<Vec<S::EntityType>>,
);

impl<S: NameStage> TypeAlternatives<S> {
    /// The alternatives `alternatives`, or `None` when there is none or one of
    /// them holds no type.
    #[must_use]
    pub fn new(alternatives: Vec<Vec<S::EntityType>>) -> Option<TypeAlternatives<S>> {
        (!alternatives.is_empty() && alternatives.iter().all(|conjunction| !conjunction.is_empty())).then_some(TypeAlternatives(alternatives))
    }

    /// The selection of the one type `entity_type`.
    #[must_use]
    pub fn single(entity_type: S::EntityType) -> TypeAlternatives<S> {
        TypeAlternatives(vec![vec![entity_type]])
    }

    /// The alternatives in the order written, each the types an entity has to
    /// carry together.
    pub fn iter(&self) -> impl Iterator<Item = &[S::EntityType]> {
        self.0.iter().map(Vec::as_slice)
    }

    /// The types the alternatives name, in order of appearance.
    pub fn leaves(&self) -> impl Iterator<Item = &S::EntityType> {
        self.0.iter().flatten()
    }

    /// The only type this selection selects, when it names exactly one.
    #[must_use]
    pub const fn as_single_type(&self) -> Option<&S::EntityType> {
        let [conjunction] = self.0.as_slice() else {
            return None;
        };
        let [only] = conjunction.as_slice() else {
            return None;
        };
        Some(only)
    }

    /// Whether `types` satisfies one of the alternatives, a conjunction needing
    /// every one of its types present. Names are compared whole.
    pub fn matches<T: AsRef<str>>(&self, types: &[T]) -> bool {
        self.iter().any(|conjunction| {
            conjunction
                .iter()
                .all(|required| types.iter().any(|candidate| candidate.as_ref() == required.as_ref()))
        })
    }

    /// The alternatives of every selection in `selections`, in order, or `None`
    /// when there is none.
    #[must_use]
    pub fn union(selections: impl IntoIterator<Item = TypeAlternatives<S>>) -> Option<TypeAlternatives<S>> {
        TypeAlternatives::new(selections.into_iter().flat_map(|selection| selection.0).collect())
    }

    /// The selection with every type replaced by `leaf`, which fails at the
    /// first type it refuses. The shape is kept, so the result is as non-empty
    /// as the selection.
    ///
    /// # Errors
    ///
    /// The first error `leaf` returns.
    pub fn try_map<U: NameStage, E>(&self, mut leaf: impl FnMut(&S::EntityType) -> Result<U::EntityType, E>) -> Result<TypeAlternatives<U>, E> {
        self.0
            .iter()
            .map(|conjunction| conjunction.iter().map(&mut leaf).collect())
            .collect::<Result<Vec<_>, E>>()
            .map(TypeAlternatives)
    }

    /// The selection with every type replaced by what the future `leaf` starts for it resolves to, which fails at the first type it refuses. The shape is kept, so the result is as non-empty as the selection.
    ///
    /// A future of `leaf` may borrow its type for as long as the selection is
    /// borrowed, one lifetime for every type rather than a fresh one per call,
    /// so stable Rust proves a caller's future `Send` from the future's own
    /// type.
    ///
    /// # Errors
    ///
    /// The first error `leaf` returns.
    pub async fn try_map_async<'s, U: NameStage, E, F>(&'s self, mut leaf: impl FnMut(&'s S::EntityType) -> F) -> Result<TypeAlternatives<U>, E>
    where
        F: Future<Output = Result<U::EntityType, E>>,
    {
        let mut conjunctions = Vec::with_capacity(self.0.len());
        for conjunction in &self.0 {
            let mut mapped = Vec::with_capacity(conjunction.len());
            for entity_type in conjunction {
                mapped.push(leaf(entity_type).await?);
            }
            conjunctions.push(mapped);
        }
        Ok(TypeAlternatives(conjunctions))
    }

    /// The alternatives as owned conjunctions.
    #[must_use]
    pub fn into_conjunctions(self) -> Vec<Vec<S::EntityType>> {
        self.0
    }
}

// A conjunction is legal only parenthesised (clause 4.17 `OrEntityType = "("
// EntityType *(";" EntityType) ")" / EntityType`) and `|` and `,` are one
// operator, so a disjunction always renders as `,`: the rendering walks two
// levels of variable-length list, which a derive cannot express.
impl<S: NameStage> Display for TypeAlternatives<S> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        for (index, conjunction) in self.iter().enumerate() {
            if index > 0 {
                formatter.write_char(ALTERNATIVE_SEPARATOR)?;
            }
            let grouped = conjunction.len() > 1;
            if grouped {
                formatter.write_char('(')?;
            }
            for (position, entity_type) in conjunction.iter().enumerate() {
                if position > 0 {
                    formatter.write_char(REQUIREMENT_SEPARATOR)?;
                }
                formatter.write_str(entity_type.as_ref())?;
            }
            if grouped {
                formatter.write_char(')')?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{name_stage::Written, type_selection::type_alternatives::TypeAlternatives};
    use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, reference::NgsiLdReferenceError};
    use std::future::ready;

    fn name(raw: &str) -> EntityTypeName {
        raw.parse().expect("valid type name")
    }

    #[test]
    fn no_alternative_and_an_empty_alternative_are_refused() {
        assert!(TypeAlternatives::<Written>::new(Vec::new()).is_none());
        assert!(TypeAlternatives::<Written>::new(vec![vec![name("A")], Vec::new()]).is_none());
        assert!(TypeAlternatives::<Written>::new(vec![vec![name("A")]]).is_some());
    }

    #[tokio::test]
    async fn an_asynchronous_map_keeps_the_shape_and_the_order() {
        let selection = TypeAlternatives::<Written>::new(vec![vec![name("A"), name("B")], vec![name("C")]]).expect("non-empty");

        let mapped = selection
            .try_map_async::<Written, NgsiLdReferenceError, _>(|entity_type| ready(entity_type.as_str().to_lowercase().parse()))
            .await
            .expect("every type maps");

        pretty_assertions::assert_eq!(mapped.to_string(), "(a;b),c");
    }

    #[tokio::test]
    async fn an_asynchronous_map_stops_at_the_first_type_it_refuses() {
        let selection = TypeAlternatives::<Written>::new(vec![vec![name("A")], vec![name("B"), name("C")]]).expect("non-empty");
        let mut visited = Vec::new();

        let refused = selection
            .try_map_async::<Written, &str, _>(|entity_type| {
                visited.push(entity_type.as_str().to_owned());
                ready(if entity_type.as_str() == "B" {
                    Err("refused")
                } else {
                    Ok(entity_type.clone())
                })
            })
            .await;

        pretty_assertions::assert_eq!(refused, Err("refused"));
        pretty_assertions::assert_eq!(visited, vec!["A".to_owned(), "B".to_owned()]);
    }

    #[test]
    fn the_union_of_selections_holds_their_alternatives_in_order() {
        let union = TypeAlternatives::<Written>::union([
            TypeAlternatives::single(name("A")),
            TypeAlternatives::new(vec![vec![name("B"), name("C")]]).expect("non-empty"),
        ]);
        pretty_assertions::assert_eq!(union.expect("two selections").to_string(), "A,(B;C)");
        assert!(TypeAlternatives::<Written>::union(Vec::new()).is_none());
    }
}
