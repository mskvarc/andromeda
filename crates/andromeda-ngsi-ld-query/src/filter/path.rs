//! Attribute paths addressed by a `q` predicate (clause 4.9).

use crate::error::QueryError;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use iri_rs::IriBuf;

/// An attribute path within a `q` expression, potentially including sub-attributes
/// and compound value navigation.
///
/// Example: `address[city].streetName` would have `attr = "address"`,
/// `sub_attrs = ["city"]`, `value_path = ["streetName"]`.
///
/// `attr`, `sub_attrs` and `value_path` are all [`AttributeName`]s, which accept
/// a compact short name, a prefixed name or an expanded IRI. That is exactly the
/// range this AST spans: a freshly parsed filter carries whatever the client
/// wrote, and an expanded one carries IRIs.
///
/// This type models the two *local* traversal axes: `.` sub-attributes and `[]`
/// compound-value navigation. The third clause 4.9 axis, the `{}` linked-entity
/// traversal, crosses an entity boundary and is modelled one level up by
/// [`QueryPath::Linked`], whose terminal segment is an `AttrPath` again.
#[derive(Debug, Clone)]
pub struct AttrPath {
    /// The top-level attribute name (compact or expanded IRI).
    pub attr: AttributeName,
    /// Sub-attribute names for navigating nested attributes.
    pub sub_attrs: Vec<AttributeName>,
    /// Member names for navigating into compound values.
    ///
    /// Typed as [`AttributeName`], not as free-form JSON keys, because that is
    /// what they address. JSON-LD expansion turns the members of a structured
    /// Property value into IRI-keyed nodes exactly as it turns attribute names
    /// into IRIs, so a member name arrives compact and must be expanded through
    /// the request `@context` before it can address anything the broker stored.
    /// A path treating them as opaque keys addressed `city` in a payload that
    /// holds `https://uri.etsi.org/ngsi-ld/default-context/city`, and matched
    /// nothing.
    pub value_path: Vec<AttributeName>,
    /// The pre-expansion spelling of each `value_path` member, same length and
    /// order.
    ///
    /// A compound value reaches the evaluator in two genuinely different
    /// shapes (see `eval::resolve_compound_member`): a structured Property
    /// value, whose members JSON-LD expansion turns into IRI-keyed nodes, and
    /// a `JsonProperty`'s (or an opaque Property's) raw JSON object, whose keys
    /// are never touched by expansion because they sit inside an opaque value,
    /// not the JSON-LD graph. Term-expanding `value_path` addresses the first
    /// shape; this field keeps what the client actually wrote so the second
    /// shape stays addressable too. Left equal to `value_path` until
    /// `expand_attr_path` overwrites `value_path` alone.
    pub value_path_compact: Vec<AttributeName>,
    /// Whether the path carried the `[*]` language-suppression marker, which
    /// matches a `LanguageProperty` against every language's value rather than
    /// against the one the `lang` parameter would otherwise pick (clause 4.9).
    pub match_all_languages: bool,
}

// Hand-written because the field to compare is chosen, not all of them:
// `value_path_compact` is a retry spelling carried alongside `value_path` for
// raw-JSON member resolution, not part of what a path addresses — two paths
// that reach the same location are equal regardless of which retry spelling
// they carry. The sanctioned derive for a skipped field is `educe`, and a
// proc-macro dependency in this crate is not worth one skipped field.
impl PartialEq for AttrPath {
    fn eq(&self, other: &AttrPath) -> bool {
        self.attr == other.attr
            && self.sub_attrs == other.sub_attrs
            && self.value_path == other.value_path
            && self.match_all_languages == other.match_all_languages
    }
}

impl Eq for AttrPath {}

impl AttrPath {
    /// Build a path addressing an attribute's own value, with no sub-attribute
    /// and no compound value navigation.
    #[must_use]
    pub const fn simple(attr: AttributeName) -> AttrPath {
        AttrPath {
            attr,
            sub_attrs: Vec::new(),
            value_path: Vec::new(),
            value_path_compact: Vec::new(),
            match_all_languages: false,
        }
    }
}

/// The attribute axis a `q` predicate compares along (clause 4.9 `Attribute`).
///
/// Clause 4.9 gives an attribute reference three axes. Two of them stay inside
/// one entity and are carried by [`AttrPath`]. The third — the
/// `LinkedEntityRelation` production, written `rel{…}` — crosses a Relationship
/// into the entity it names, and may be chained. This enum is that distinction:
/// a path is either wholly local, or one hop followed by a path that is itself
/// either local or a further hop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryPath {
    /// A path addressing an attribute of the entity under evaluation.
    Local(AttrPath),
    /// A hop across a Relationship into the entity it links to.
    Linked {
        /// The Relationship the hop descends through.
        relationship: AttributeName,
        /// Entity types the linked entity is constrained to, empty when the hop
        /// carried no constraint.
        ///
        /// The clause 4.9 production is
        /// `*1(EntityType 1*("," EntityType) ":")`, so a constraint needs two or
        /// more comma-separated types. That is what keeps the grammar
        /// unambiguous: a lone `prefix:name` inside the braces is a prefixed
        /// attribute name, never a one-element type constraint.
        guard_types: Vec<AttributeName>,
        /// The path evaluated against the linked entity.
        inner: Box<QueryPath>,
    },
}

impl QueryPath {
    /// The local path this axis terminates at — the attribute whose value the
    /// predicate ultimately compares.
    ///
    /// For a hop chain that is the innermost segment, which is the attribute a
    /// value-level concern such as `expandValues` or `jsonKeys` (clause 6.4.3.2)
    /// names: those parameters describe the values being compared, and after a
    /// hop the compared values belong to the linked entity's attribute.
    #[must_use]
    pub fn terminal(&self) -> &AttrPath {
        match self {
            QueryPath::Local(local) => local,
            QueryPath::Linked { inner, .. } => inner.terminal(),
        }
    }

    /// Whether this axis crosses into a linked entity.
    ///
    /// Testing the outermost segment is enough to answer that for the whole
    /// axis: a hop wraps everything after it, so a [`QueryPath::Local`] can
    /// never have one nested inside.
    #[must_use]
    pub const fn is_linked(&self) -> bool {
        matches!(self, QueryPath::Linked { .. })
    }

    /// How many Relationship hops this axis crosses before it lands on a local
    /// attribute.
    ///
    /// Zero for a wholly local path, one for `rel{attr}`, two for
    /// `rel{rel2{attr}}`, and so on. Clause 5.7.2.4 measures a filter against
    /// `joinLevel` by exactly this count, so the number the traversal is bounded
    /// by and the number the rule checks are the same one.
    #[must_use]
    pub fn linked_depth(&self) -> u32 {
        match self {
            QueryPath::Local(_) => 0,
            QueryPath::Linked { inner, .. } => 1 + inner.linked_depth(),
        }
    }

    /// The local path this axis terminates at, mutably.
    pub fn terminal_mut(&mut self) -> &mut AttrPath {
        match self {
            QueryPath::Local(local) => local,
            QueryPath::Linked { inner, .. } => inner.terminal_mut(),
        }
    }
}

/// Read an expanded IRI as the attribute name that addresses it.
///
/// Term expansion yields an absolute IRI, which the clause 4.6.2 name grammar
/// accepts, so the failure path only opens if a context produced something an
/// attribute cannot be keyed by.
pub fn expanded_attribute_name(iri: &IriBuf) -> Result<AttributeName, QueryError> {
    iri.as_str().parse().map_err(|source| QueryError::AttributeNameInvalid {
        value: iri.as_str().to_string(),
        source,
    })
}
