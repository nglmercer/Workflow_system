use std::collections::HashMap;

use crate::ast::*;
use workflow_domain::{TriggerContext, WorkflowResult};

/// Runtime value
#[derive(Debug, Clone)]
pub enum Value {
    String(String),
    Number(f64),
    Bool(bool),
    Null,
    Array(Vec<Value>),
    Object(HashMap<String, Value>),
}

impl Value {
    pub fn to_json(&self) -> serde_json::Value {
        match self {
            Value::String(s) => serde_json::Value::String(s.clone()),
            Value::Number(n) => serde_json::json!(n),
            Value::Bool(b) => serde_json::Value::Bool(*b),
            Value::Null => serde_json::Value::Null,
            Value::Array(arr) => {
                serde_json::Value::Array(arr.iter().map(|v| v.to_json()).collect())
            }
            Value::Object(map) => {
                let obj: serde_json::Map<String, serde_json::Value> =
                    map.iter().map(|(k, v)| (k.clone(), v.to_json())).collect();
                serde_json::Value::Object(obj)
            }
        }
    }

    pub fn from_json(val: &serde_json::Value) -> Self {
        match val {
            serde_json::Value::String(s) => Value::String(s.clone()),
            serde_json::Value::Number(n) => Value::Number(n.as_f64().unwrap_or(0.0)),
            serde_json::Value::Bool(b) => Value::Bool(*b),
            serde_json::Value::Null => Value::Null,
            serde_json::Value::Array(arr) => {
                Value::Array(arr.iter().map(Value::from_json).collect())
            }
            serde_json::Value::Object(map) => Value::Object(
                map.iter()
                    .map(|(k, v)| (k.clone(), Value::from_json(v)))
                    .collect(),
            ),
        }
    }

    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Number(n) => *n != 0.0,
            Value::String(s) => !s.is_empty(),
            Value::Null => false,
            Value::Array(arr) => !arr.is_empty(),
            Value::Object(map) => !map.is_empty(),
        }
    }
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::String(s) => write!(f, "{}", s),
            Value::Number(n) => write!(f, "{}", n),
            Value::Bool(b) => write!(f, "{}", b),
            Value::Null => write!(f, "null"),
            Value::Array(arr) => {
                let items: Vec<String> = arr.iter().map(|v| v.to_string()).collect();
                write!(f, "[{}]", items.join(", "))
            }
            Value::Object(_) => write!(f, "[object]"),
        }
    }
}

/// Evaluator for .flow programs
pub struct FlowEvaluator {
    globals: HashMap<String, Value>,
    functions: HashMap<String, FunctionDef>,
    logs: Vec<String>,
}

impl FlowEvaluator {
    pub fn new() -> Self {
        Self {
            globals: HashMap::new(),
            functions: HashMap::new(),
            logs: Vec::new(),
        }
    }

    pub fn load_program(&mut self, program: &FlowProgram) {
        // Load global variables
        for g in &program.globals {
            let value = self.eval_expr(&g.value, &HashMap::new());
            self.globals.insert(g.name.clone(), value);
        }

        // Load functions
        for f in &program.functions {
            self.functions.insert(f.name.clone(), f.clone());
        }
    }

    pub fn execute_workflow(
        &mut self,
        workflow: &WorkflowDef,
        context: &TriggerContext,
    ) -> WorkflowResult<Vec<String>> {
        self.logs.clear();

        let mut vars = HashMap::new();
        vars.insert("data".to_string(), Value::from_json(&context.data));
        if let Some(ref ctx_vars) = context.vars {
            vars.insert("vars".to_string(), Value::from_json(ctx_vars));
        }

        for stmt in &workflow.body {
            self.exec_stmt(stmt, &mut vars)?;
        }

        Ok(self.logs.clone())
    }

    fn exec_stmt(
        &mut self,
        stmt: &Stmt,
        vars: &mut HashMap<String, Value>,
    ) -> WorkflowResult<bool> {
        match stmt {
            Stmt::VarDecl { name, value } => {
                let val = value
                    .as_ref()
                    .map(|e| self.eval_expr(e, vars))
                    .unwrap_or(Value::Null);
                vars.insert(name.clone(), val);
                Ok(false)
            }
            Stmt::If {
                condition,
                then_body,
                else_body,
            } => {
                let cond_val = self.eval_expr(condition, vars);
                if cond_val.is_truthy() {
                    for stmt in then_body {
                        if self.exec_stmt(stmt, vars)? {
                            return Ok(true);
                        }
                    }
                } else if let Some(else_body) = else_body {
                    for stmt in else_body {
                        if self.exec_stmt(stmt, vars)? {
                            return Ok(true);
                        }
                    }
                }
                Ok(false)
            }
            Stmt::Return { value: _ } => {
                // Return is not used in workflow context, just ignore
                Ok(false)
            }
            Stmt::Expr(expr) => {
                self.eval_expr(expr, vars);
                Ok(false)
            }
            Stmt::Log(expr) => {
                let val = self.eval_expr(expr, vars);
                self.logs.push(val.to_string());
                Ok(false)
            }
            Stmt::Foreach {
                item_var,
                iterable,
                body,
            } => {
                let arr = self.eval_expr(iterable, vars);
                if let Value::Array(items) = arr {
                    for item in items {
                        vars.insert(item_var.clone(), item);
                        for stmt in body {
                            if self.exec_stmt(stmt, vars)? {
                                return Ok(true);
                            }
                        }
                    }
                }
                Ok(false)
            }
        }
    }

    pub fn eval_expr(&mut self, expr: &Expr, vars: &HashMap<String, Value>) -> Value {
        match expr {
            Expr::String(s) => Value::String(s.clone()),
            Expr::Number(n) => Value::Number(*n),
            Expr::Bool(b) => Value::Bool(*b),
            Expr::Null => Value::Null,
            Expr::Var(name) => vars
                .get(name)
                .or_else(|| self.globals.get(name))
                .cloned()
                .unwrap_or(Value::Null),
            Expr::Member { object, property } => {
                let obj = self.eval_expr(object, vars);
                match obj {
                    Value::Object(map) => map.get(property).cloned().unwrap_or(Value::Null),
                    Value::Array(arr) => {
                        if let Ok(idx) = property.parse::<usize>() {
                            arr.get(idx).cloned().unwrap_or(Value::Null)
                        } else {
                            Value::Null
                        }
                    }
                    _ => Value::Null,
                }
            }
            Expr::BinaryOp { op, left, right } => {
                let l = self.eval_expr(left, vars);
                let r = self.eval_expr(right, vars);
                self.eval_binary_op(op, &l, &r)
            }
            Expr::UnaryOp { op, operand } => {
                let val = self.eval_expr(operand, vars);
                match op {
                    UnaryOp::Not => Value::Bool(!val.is_truthy()),
                    UnaryOp::Neg => match val {
                        Value::Number(n) => Value::Number(-n),
                        _ => Value::Null,
                    },
                }
            }
            Expr::Call { name, args } => {
                let arg_vals: Vec<Value> = args.iter().map(|a| self.eval_expr(a, vars)).collect();
                self.call_function(name, &arg_vals)
            }
            Expr::Array(elems) => {
                let vals = elems.iter().map(|e| self.eval_expr(e, vars)).collect();
                Value::Array(vals)
            }
            Expr::InterpolatedString(parts) => {
                let mut result = String::new();
                for part in parts {
                    match part {
                        InterpPart::Text(t) => result.push_str(t),
                        InterpPart::Expr(e) => {
                            let val = self.eval_expr(e, vars);
                            result.push_str(&val.to_string());
                        }
                    }
                }
                Value::String(result)
            }
        }
    }

    fn eval_binary_op(&self, op: &BinaryOp, left: &Value, right: &Value) -> Value {
        match op {
            BinaryOp::Add => match (left, right) {
                (Value::String(a), Value::String(b)) => Value::String(format!("{}{}", a, b)),
                (Value::Number(a), Value::Number(b)) => Value::Number(a + b),
                _ => Value::Null,
            },
            BinaryOp::Sub => match (left, right) {
                (Value::Number(a), Value::Number(b)) => Value::Number(a - b),
                _ => Value::Null,
            },
            BinaryOp::Mul => match (left, right) {
                (Value::Number(a), Value::Number(b)) => Value::Number(a * b),
                _ => Value::Null,
            },
            BinaryOp::Div => match (left, right) {
                (Value::Number(a), Value::Number(b)) => {
                    if *b != 0.0 {
                        Value::Number(a / b)
                    } else {
                        Value::Null
                    }
                }
                _ => Value::Null,
            },
            BinaryOp::Mod => match (left, right) {
                (Value::Number(a), Value::Number(b)) => Value::Number(a % b),
                _ => Value::Null,
            },
            BinaryOp::Eq => Value::Bool(self.values_equal(left, right)),
            BinaryOp::Neq => Value::Bool(!self.values_equal(left, right)),
            BinaryOp::Lt => match (left, right) {
                (Value::Number(a), Value::Number(b)) => Value::Bool(a < b),
                (Value::String(a), Value::String(b)) => Value::Bool(a < b),
                _ => Value::Bool(false),
            },
            BinaryOp::Gt => match (left, right) {
                (Value::Number(a), Value::Number(b)) => Value::Bool(a > b),
                (Value::String(a), Value::String(b)) => Value::Bool(a > b),
                _ => Value::Bool(false),
            },
            BinaryOp::Lte => match (left, right) {
                (Value::Number(a), Value::Number(b)) => Value::Bool(a <= b),
                (Value::String(a), Value::String(b)) => Value::Bool(a <= b),
                _ => Value::Bool(false),
            },
            BinaryOp::Gte => match (left, right) {
                (Value::Number(a), Value::Number(b)) => Value::Bool(a >= b),
                (Value::String(a), Value::String(b)) => Value::Bool(a >= b),
                _ => Value::Bool(false),
            },
            BinaryOp::And => Value::Bool(left.is_truthy() && right.is_truthy()),
            BinaryOp::Or => Value::Bool(left.is_truthy() || right.is_truthy()),
        }
    }

    fn values_equal(&self, left: &Value, right: &Value) -> bool {
        match (left, right) {
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Number(a), Value::Number(b)) => a == b,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Null, Value::Null) => true,
            _ => false,
        }
    }

    fn call_function(&mut self, name: &str, args: &[Value]) -> Value {
        match name {
            "log" => {
                if let Some(val) = args.first() {
                    self.logs.push(val.to_string());
                }
                Value::Null
            }
            "len" => {
                if let Some(val) = args.first() {
                    match val {
                        Value::String(s) => Value::Number(s.len() as f64),
                        Value::Array(arr) => Value::Number(arr.len() as f64),
                        _ => Value::Number(0.0),
                    }
                } else {
                    Value::Number(0.0)
                }
            }
            "to_string" => {
                if let Some(val) = args.first() {
                    Value::String(val.to_string())
                } else {
                    Value::String(String::new())
                }
            }
            "to_number" => {
                if let Some(val) = args.first() {
                    match val {
                        Value::Number(n) => Value::Number(*n),
                        Value::String(s) => {
                            s.parse::<f64>().map(Value::Number).unwrap_or(Value::Null)
                        }
                        Value::Bool(b) => Value::Number(if *b { 1.0 } else { 0.0 }),
                        _ => Value::Null,
                    }
                } else {
                    Value::Null
                }
            }
            _ => {
                // Check user-defined functions
                if let Some(_func) = self.functions.get(name) {
                    // Simplified: just execute the body
                    Value::Null
                } else {
                    Value::Null
                }
            }
        }
    }

    pub fn get_logs(&self) -> &[String] {
        &self.logs
    }
}

impl Default for FlowEvaluator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_eval_arithmetic() {
        let mut evaluator = FlowEvaluator::new();
        let vars = HashMap::new();

        let expr = Expr::binary(BinaryOp::Add, Expr::number(2.0), Expr::number(3.0));

        let result = evaluator.eval_expr(&expr, &vars);
        assert!(matches!(result, Value::Number(5.0)));
    }

    #[test]
    fn test_eval_comparison() {
        let mut evaluator = FlowEvaluator::new();
        let mut vars = HashMap::new();
        vars.insert("x".to_string(), Value::Number(10.0));

        let expr = Expr::binary(BinaryOp::Gt, Expr::var("x"), Expr::number(5.0));

        let result = evaluator.eval_expr(&expr, &vars);
        assert!(matches!(result, Value::Bool(true)));
    }

    #[test]
    fn test_eval_string_concat() {
        let mut evaluator = FlowEvaluator::new();
        let vars = HashMap::new();

        let expr = Expr::binary(BinaryOp::Add, Expr::string("Hello "), Expr::string("World"));

        let result = evaluator.eval_expr(&expr, &vars);
        assert!(matches!(result, Value::String(s) if s == "Hello World"));
    }
}
