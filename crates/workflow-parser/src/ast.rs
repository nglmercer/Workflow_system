use serde::{Deserialize, Serialize};

/// Top-level AST node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowProgram {
    pub globals: Vec<GlobalVar>,
    pub functions: Vec<FunctionDef>,
    pub workflows: Vec<WorkflowDef>,
}

/// Global variable declaration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalVar {
    pub name: String,
    pub value: Expr,
}

/// Function definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDef {
    pub name: String,
    pub params: Vec<String>,
    pub body: Vec<Stmt>,
}

/// Workflow definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDef {
    pub name: String,
    pub event: String,
    pub params: Vec<String>,
    pub body: Vec<Stmt>,
}

/// Statement types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Stmt {
    VarDecl {
        name: String,
        value: Option<Expr>,
    },
    If {
        condition: Expr,
        then_body: Vec<Stmt>,
        else_body: Option<Vec<Stmt>>,
    },
    Return {
        value: Option<Expr>,
    },
    Expr(Expr),
    Log(Expr),
    Foreach {
        item_var: String,
        iterable: Expr,
        body: Vec<Stmt>,
    },
    On(String),
}

/// Expression types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Expr {
    // Literals
    String(String),
    Number(f64),
    Bool(bool),
    Null,

    // Variables and references
    Var(String),
    Member {
        object: Box<Expr>,
        property: String,
    },

    // Operations
    BinaryOp {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    UnaryOp {
        op: UnaryOp,
        operand: Box<Expr>,
    },

    // Function call
    Call {
        name: String,
        args: Vec<Expr>,
    },

    // Array
    Array(Vec<Expr>),

    // Interpolated string
    InterpolatedString(Vec<InterpPart>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InterpPart {
    Text(String),
    Expr(Expr),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Neq,
    Lt,
    Gt,
    Lte,
    Gte,
    And,
    Or,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UnaryOp {
    Not,
    Neg,
}

impl Expr {
    pub fn string(s: impl Into<String>) -> Self {
        Self::String(s.into())
    }

    pub fn number(n: f64) -> Self {
        Self::Number(n)
    }

    pub fn var(name: impl Into<String>) -> Self {
        Self::Var(name.into())
    }

    pub fn call(name: impl Into<String>, args: Vec<Expr>) -> Self {
        Self::Call {
            name: name.into(),
            args,
        }
    }

    pub fn member(object: Expr, property: impl Into<String>) -> Self {
        Self::Member {
            object: Box::new(object),
            property: property.into(),
        }
    }

    pub fn binary(op: BinaryOp, left: Expr, right: Expr) -> Self {
        Self::BinaryOp {
            op,
            left: Box::new(left),
            right: Box::new(right),
        }
    }
}
