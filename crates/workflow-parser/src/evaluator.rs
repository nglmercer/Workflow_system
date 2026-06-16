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

    pub fn len(&self) -> usize {
        match self {
            Value::String(s) => s.len(),
            Value::Array(arr) => arr.len(),
            Value::Object(map) => map.len(),
            _ => 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
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

/// Result of running a workflow. Returned by
/// [`FlowEvaluator::execute_workflow_with_result`] and consumed by
/// the test runner to evaluate `expect` clauses.
#[derive(Debug, Clone)]
pub struct WorkflowOutcome {
    /// The captured log strings, in emission order.
    pub logs: Vec<String>,
    /// The value produced by the last `return` statement, or
    /// `Value::Null` if the workflow fell off the end with no
    /// `return`.
    pub return_value: Value,
    /// The full variable scope at the moment execution stopped.
    /// Includes the synthetic `data` and (when present) `vars`
    /// bindings, the destructured event params, and every
    /// `var` declared anywhere in the body (including inside
    /// `if`/`foreach` branches that ran).
    pub scope: HashMap<String, Value>,
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
        for g in &program.globals {
            let value = self.eval_expr(&g.value, &HashMap::new());
            self.globals.insert(g.name.clone(), value);
        }

        for f in &program.functions {
            self.functions.insert(f.name.clone(), f.clone());
        }
    }

    /// Insert pre-resolved bindings into the global scope. Used
    /// by the test runner to inject the payload of every
    /// `@import <name> from "<file>.json"` declaration in the
    /// host program. Last writer wins on duplicate names, which
    /// matches how the parser already handles duplicate
    /// `imports` entries.
    ///
    /// The runner calls this *after* `load_program`, so a
    /// hand-written global with the same name would be
    /// overridden by an `@import`. That's intentional: the
    /// `@import` is the test-time source of truth for
    /// externally-shaped bindings.
    pub fn populate_globals(&mut self, globals: HashMap<String, Value>) {
        for (k, v) in globals {
            self.globals.insert(k, v);
        }
    }

    pub fn execute_workflow(
        &mut self,
        workflow: &WorkflowDef,
        context: &TriggerContext,
    ) -> WorkflowResult<Vec<String>> {
        let outcome = self.execute_workflow_with_result(workflow, context)?;
        Ok(outcome.logs)
    }

    /// Execute a workflow and return the captured logs, the value
    /// produced by the last `return` statement (or `Null` if the
    /// workflow fell off the end), and a snapshot of the final
    /// variable scope. This is the rich entry point used by the
    /// test runner; [`execute_workflow`] remains for callers that
    /// only need the logs.
    pub fn execute_workflow_with_result(
        &mut self,
        workflow: &WorkflowDef,
        context: &TriggerContext,
    ) -> WorkflowResult<WorkflowOutcome> {
        self.logs.clear();

        let mut vars = HashMap::new();
        vars.insert("data".to_string(), Value::from_json(&context.data));
        if let Some(ref ctx_vars) = context.vars {
            vars.insert("vars".to_string(), Value::from_json(ctx_vars));
        }

        // If workflow has destructuring params, extract them from
        // the event payload. Two forms are supported:
        //   ({a, b, c})  — multi-binding: each name is a
        //                  field of the event payload.
        //   (name)       — single-binding: the whole event
        //                  payload is rebound to `name`. The
        //                  parser appends a `_rename` marker
        //                  to `workflow.params` to signal this
        //                  form, which we strip here before
        //                  inserting into the scope.
        if !workflow.params.is_empty() {
            let is_rename = workflow.params.last().is_some_and(|p| p == "_rename");
            let names: Vec<String> = if is_rename {
                workflow.params[..workflow.params.len() - 1].to_vec()
            } else {
                workflow.params.clone()
            };
            if is_rename {
                // `name` is a rename of the event payload. Skip
                // field extraction and just bind the whole event
                // to that name. (If `name == "data"`, the
                // earlier `vars.insert("data", ...)` is a no-op
                // for our purposes — same value.)
                for name in &names {
                    vars.insert(name.clone(), Value::from_json(&context.data));
                }
            } else if let Value::Object(ref data_map) = Value::from_json(&context.data) {
                eprintln!("DEBUG destructure data_map={:?} names={:?}", data_map, names);
                for param in &names {
                    let val = data_map.get(param).cloned().unwrap_or(Value::Null);
                    eprintln!("DEBUG destructure param={} val={:?}", param, val);
                    vars.insert(param.clone(), val);
                }
            }
        }

        let mut ret: Option<Value> = None;
        for stmt in &workflow.body {
            if let Some(value) = self.exec_stmt(stmt, &mut vars)? {
                ret = Some(value);
                break;
            }
        }

        Ok(WorkflowOutcome {
            logs: self.logs.clone(),
            return_value: ret.unwrap_or(Value::Null),
            scope: vars,
        })
    }

    /// Execute a single statement. Returns:
    /// - `Ok(Some(value))` if the statement was a `return` — the
    ///   caller should short-circuit the rest of the workflow.
    /// - `Ok(None)` for any other statement (including non-returning
    ///   control flow).
    fn exec_stmt(
        &mut self,
        stmt: &Stmt,
        vars: &mut HashMap<String, Value>,
    ) -> WorkflowResult<Option<Value>> {
        match stmt {
            Stmt::VarDecl { name, value } => {
                let val = value
                    .as_ref()
                    .map(|e| self.eval_expr(e, vars))
                    .unwrap_or(Value::Null);
                vars.insert(name.clone(), val);
                Ok(None)
            }
            Stmt::Assign { name, value } => {
                let val = self.eval_expr(value, vars);
                // Assigning to an unbound variable is a no-op
                // (the var gets created with the assigned value).
                // This matches how `vars.insert` behaves and
                // keeps the test runner's `expect var x == ...`
                // contract simple: write-then-read always
                // produces the written value.
                vars.insert(name.clone(), val);
                Ok(None)
            }
            Stmt::If {
                condition,
                then_body,
                else_body,
            } => {
                let cond_val = self.eval_expr(condition, vars);
                let branch = if cond_val.is_truthy() {
                    then_body
                } else {
                    else_body.as_deref().unwrap_or(&[])
                };
                for stmt in branch {
                    if let Some(value) = self.exec_stmt(stmt, vars)? {
                        return Ok(Some(value));
                    }
                }
                Ok(None)
            }
            Stmt::Return { value } => {
                let val = value
                    .as_ref()
                    .map(|e| self.eval_expr(e, vars))
                    .unwrap_or(Value::Null);
                Ok(Some(val))
            }
            Stmt::Expr(expr) => {
                self.eval_expr(expr, vars);
                Ok(None)
            }
            Stmt::Log(expr) => {
                let val = self.eval_expr(expr, vars);
                self.logs.push(val.to_string());
                Ok(None)
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
                            if let Some(value) = self.exec_stmt(stmt, vars)? {
                                return Ok(Some(value));
                            }
                        }
                    }
                }
                Ok(None)
            }
            Stmt::On { .. } => {
                // On is handled at workflow level, skip during body execution
                Ok(None)
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
                match &obj {
                    Value::Object(map) => map.get(property).cloned().unwrap_or(Value::Null),
                    Value::Array(arr) => {
                        if property == "length" {
                            Value::Number(arr.len() as f64)
                        } else if let Ok(idx) = property.parse::<usize>() {
                            arr.get(idx).cloned().unwrap_or(Value::Null)
                        } else {
                            Value::Null
                        }
                    }
                    Value::String(s) => {
                        if property == "length" {
                            Value::Number(s.len() as f64)
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
                (Value::String(a), b) => Value::String(format!("{}{}", a, b)),
                (a, Value::String(b)) => Value::String(format!("{}{}", a, b)),
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
                    Value::Number(val.len() as f64)
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
                if let Some(func) = self.functions.get(name).cloned() {
                    let mut local_vars = HashMap::new();
                    for (i, param) in func.params.iter().enumerate() {
                        let val = args.get(i).cloned().unwrap_or(Value::Null);
                        local_vars.insert(param.clone(), val);
                    }
                    let mut result = Value::Null;
                    for stmt in &func.body {
                        match self.exec_stmt(stmt, &mut local_vars) {
                            Ok(Some(value)) => {
                                result = value;
                                break;
                            }
                            Ok(None) => continue,
                            Err(_) => break,
                        }
                    }
                    result
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

    #[test]
    fn test_eval_array_length() {
        let mut evaluator = FlowEvaluator::new();
        let mut vars = HashMap::new();
        let mut obj = HashMap::new();
        obj.insert(
            "items".to_string(),
            Value::Array(vec![
                Value::Number(1.0),
                Value::Number(2.0),
                Value::Number(3.0),
            ]),
        );
        vars.insert("data".to_string(), Value::Object(obj));

        let expr = Expr::member(Expr::member(Expr::var("data"), "items"), "length");
        let result = evaluator.eval_expr(&expr, &vars);
        assert!(matches!(result, Value::Number(3.0)));
    }

    #[test]
    fn test_eval_string_length() {
        let mut evaluator = FlowEvaluator::new();
        let mut vars = HashMap::new();
        vars.insert("name".to_string(), Value::String("hello".to_string()));

        let expr = Expr::member(Expr::var("name"), "length");
        let result = evaluator.eval_expr(&expr, &vars);
        assert!(matches!(result, Value::Number(5.0)));
    }
}
