use serde_json::Number;

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    And(Vec<Expr>),
    Or(Vec<Expr>),
    Pred(Predicate),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Predicate {
    Exists(AttrPath),
    Compare { path: AttrPath, op: CmpOp, rhs: CompareValue },
    MatchPattern { path: AttrPath, pattern: String },
    NotMatchPattern { path: AttrPath, pattern: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct AttrPath {
    pub attr: String,
    pub sub_attrs: Vec<String>,
    pub value_path: Vec<String>,
}

#[allow(dead_code)]
impl AttrPath {
    pub fn simple(attr: impl Into<String>) -> Self {
        Self {
            attr: attr.into(),
            sub_attrs: Vec::new(),
            value_path: Vec::new(),
        }
    }

    pub fn with_sub_attrs(mut self, sub_attrs: Vec<String>) -> Self {
        self.sub_attrs = sub_attrs;
        self
    }

    pub fn with_value_path(mut self, value_path: Vec<String>) -> Self {
        self.value_path = value_path;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    Equal,
    Unequal,
    Greater,
    GreaterEq,
    Less,
    LessEq,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CompareValue {
    Scalar(Scalar),
    Range { min: Scalar, max: Scalar },
    ValueList(Vec<Scalar>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Scalar {
    Number(Number),
    Bool(bool),
    Str(String),
    DateTime(String),
    Date(String),
    Time(String),
}

#[allow(dead_code)]
impl Scalar {
    pub fn is_numeric(&self) -> bool {
        matches!(self, Scalar::Number(_))
    }
}
