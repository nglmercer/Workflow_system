use std::collections::HashMap;

use crate::ast::*;
use workflow_domain::{TriggerContext, WorkflowResult};

/// Runtime value
#[derive(Debug, Clone, PartialEq)]
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
    /// The events emitted via `emit("EVENT")`, in call order.
    pub emitted: Vec<String>,
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
    /// Errors collected during execution (e.g., undefined function calls).
    pub errors: Vec<String>,
}

/// A native function callable from `.flow` files.
/// Receives arguments as `serde_json::Value` and returns a value.
pub type NativeFunction = Box<dyn Fn(&[serde_json::Value]) -> serde_json::Value + Send + Sync>;

/// An object getter for `${plugin_name.path}` access in `.flow` files.
/// Receives a dot-separated path and returns the value at that path.
pub type ObjectGetter = Box<dyn Fn(&str) -> Option<serde_json::Value> + Send + Sync>;

/// Evaluator for .flow programs
pub struct FlowEvaluator {
    globals: HashMap<String, Value>,
    functions: HashMap<String, FunctionDef>,
    /// Native functions registered by plugins (e.g., `http_get`, `csv_parse`).
    native_functions: HashMap<String, NativeFunction>,
    /// Object getters registered by plugins (e.g., `config` for `${config.base_url}`).
    object_getters: HashMap<String, ObjectGetter>,
    logs: Vec<String>,
    emitted: Vec<String>,
    /// Errors collected during execution (e.g., undefined function calls).
    errors: Vec<String>,
}

impl FlowEvaluator {
    pub fn new() -> Self {
        Self {
            globals: HashMap::new(),
            functions: HashMap::new(),
            native_functions: HashMap::new(),
            object_getters: HashMap::new(),
            logs: Vec::new(),
            emitted: Vec::new(),
            errors: Vec::new(),
        }
    }

    /// Register a native function callable from `.flow` files.
    ///
    /// # Example
    /// ```ignore
    /// evaluator.register_native_function(
    ///     "http_get",
    ///     Box::new(|args| {
    ///         let url = args.first().unwrap().as_str().unwrap_or("");
    ///         serde_json::json!({ "status": 200, "body": "..." })
    ///     }),
    /// );
    /// ```
    pub fn register_native_function(&mut self, name: &str, func: NativeFunction) {
        self.native_functions.insert(name.to_string(), func);
    }

    /// Register an object getter for `${plugin_name.path}` access.
    ///
    /// # Example
    /// ```ignore
    /// evaluator.register_object_getter(
    ///     "config",
    ///     Box::new(|path| {
    ///         match path {
    ///             "base_url" => Some(serde_json::json!("https://api.example.com")),
    ///             _ => None,
    ///         }
    ///     }),
    /// );
    /// ```
    pub fn register_object_getter(&mut self, plugin_name: &str, getter: ObjectGetter) {
        self.object_getters.insert(plugin_name.to_string(), getter);
    }

    /// Set multiple native functions at once.
    pub fn set_native_functions(&mut self, functions: HashMap<String, NativeFunction>) {
        self.native_functions.extend(functions);
    }

    /// Set multiple object getters at once.
    pub fn set_object_getters(&mut self, getters: HashMap<String, ObjectGetter>) {
        self.object_getters.extend(getters);
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
        self.emitted.clear();

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
                for param in &names {
                    let val = data_map.get(param).cloned().unwrap_or(Value::Null);
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
            emitted: self.emitted.clone(),
            return_value: ret.unwrap_or(Value::Null),
            scope: vars,
            errors: self.errors.clone(),
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
            Stmt::VarDecl { name, value, .. } => {
                let val = value
                    .as_ref()
                    .map(|e| self.eval_expr(e, vars))
                    .unwrap_or(Value::Null);
                vars.insert(name.clone(), val);
                Ok(None)
            }
            Stmt::Assign { name, value, .. } => {
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
                ..
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
            Stmt::Return { value, .. } => {
                let val = value
                    .as_ref()
                    .map(|e| self.eval_expr(e, vars))
                    .unwrap_or(Value::Null);
                Ok(Some(val))
            }
            Stmt::Expr(expr, _) => {
                self.eval_expr(expr, vars);
                Ok(None)
            }
            Stmt::Log(expr, _) => {
                let val = self.eval_expr(expr, vars);
                self.logs.push(val.to_string());
                Ok(None)
            }
            Stmt::Foreach {
                item_var,
                iterable,
                body,
                ..
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
                .or_else(|| {
                    // Check object getters: `${plugin_name}` resolves to the
                    // getter's root object. If the getter returns a value,
                    // wrap it in a Value; otherwise return Null.
                    if let Some(getter) = self.object_getters.get(name) {
                        getter("").map(|val| Value::from_json(&val))
                    } else {
                        None
                    }
                })
                .unwrap_or(Value::Null),
            Expr::Member { object, property } => {
                let obj = self.eval_expr(object, vars);

                // If the object is a string variable name matching a registered
                // plugin, try to resolve via the object getter.
                // e.g., `${config.base_url}` -> Expr::Member(Var("config"), "base_url")
                if let Expr::Var(name) = object.as_ref() {
                    if self.object_getters.contains_key(name) {
                        if let Some(getter) = self.object_getters.get(name) {
                            if let Some(val) = getter(property) {
                                return Value::from_json(&val);
                            }
                        }
                    }
                }

                match &obj {
                    Value::Object(map) => {
                        // `meta.length` on an object returns the
                        // number of fields, matching how arrays
                        // and strings work. Without this, the
                        // test runner's `expect var x == ...`
                        // against a destructured object would
                        // always see `Null` for `length`.
                        if property == "length" {
                            Value::Number(map.len() as f64)
                        } else {
                            map.get(property).cloned().unwrap_or(Value::Null)
                        }
                    }
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
        // First check native functions registered by plugins
        if let Some(getter) = self.native_functions.get(name) {
            // Convert Value args to serde_json::Value for the native function
            let json_args: Vec<serde_json::Value> = args.iter().map(|v| v.to_json()).collect();
            let result = getter(&json_args);
            return Value::from_json(&result);
        }

        match name {
            "log" => {
                if let Some(val) = args.first() {
                    self.logs.push(val.to_string());
                }
                Value::Null
            }
            "emit" => {
                if let Some(Value::String(event)) = args.first() {
                    self.emitted.push(event.clone());
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
                    self.errors.push(format!(
                        "Undefined function '{}' — it is not a built-in, \
                         user-defined, or registered plugin function",
                        name
                    ));
                    Value::Null
                }
            }
        }
    }

    pub fn get_logs(&self) -> &[String] {
        &self.logs
    }

    pub fn get_emitted(&self) -> &[String] {
        &self.emitted
    }

    /// Returns errors collected during execution (e.g., undefined
    /// function calls).  An empty Vec means no errors occurred.
    pub fn get_errors(&self) -> &[String] {
        &self.errors
    }

    /// Merge another program's globals and functions into this
    /// evaluator. Functions from `other` are added only if the
    /// name doesn't already exist (first-writer-wins). Globals
    /// from `other` are added only if the name doesn't already
    /// exist. This supports cross-file imports where a `.flow`
    /// file imports another `.flow` file for shared functions.
    pub fn merge_program(&mut self, other: &FlowProgram) {
        for f in &other.functions {
            self.functions
                .entry(f.name.clone())
                .or_insert_with(|| f.clone());
        }
        for g in &other.globals {
            if !self.globals.contains_key(&g.name) {
                let value = self.eval_expr(&g.value, &HashMap::new());
                self.globals.insert(g.name.clone(), value);
            }
        }
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

    #[test]
    fn test_emit_records_event() {
        let mut evaluator = FlowEvaluator::new();
        let args = vec![Value::String("USER_ACTIVATED".to_string())];

        let result = evaluator.call_function("emit", &args);
        assert!(matches!(result, Value::Null));
        assert_eq!(evaluator.get_emitted(), &["USER_ACTIVATED"]);
    }

    #[test]
    fn test_emit_multiple_events() {
        let mut evaluator = FlowEvaluator::new();

        evaluator.call_function("emit", &[Value::String("EVENT_A".to_string())]);
        evaluator.call_function("emit", &[Value::String("EVENT_B".to_string())]);
        evaluator.call_function("emit", &[Value::String("EVENT_C".to_string())]);

        assert_eq!(evaluator.get_emitted(), &["EVENT_A", "EVENT_B", "EVENT_C"]);
    }

    #[test]
    fn test_emit_with_non_string_is_ignored() {
        let mut evaluator = FlowEvaluator::new();

        evaluator.call_function("emit", &[Value::Number(42.0)]);
        evaluator.call_function("emit", &[Value::Bool(true)]);
        evaluator.call_function("emit", &[Value::Null]);

        assert!(evaluator.get_emitted().is_empty());
    }

    #[test]
    fn test_merge_program_adds_functions() {
        let mut evaluator = FlowEvaluator::new();

        let other = FlowProgram {
            imports: vec![],
            globals: vec![],
            functions: vec![FunctionDef {
                name: "greet".to_string(),
                params: vec!["name".to_string()],
                body: vec![Stmt::Log(
                    Expr::binary(BinaryOp::Add, Expr::string("Hello "), Expr::var("name")),
                    Span::default(),
                )],
                span: Span::default(),
            }],
            workflows: vec![],
            tests: vec![],
            span: Span::default(),
        };

        evaluator.merge_program(&other);

        // The function should now be callable
        let args = vec![Value::String("World".to_string())];
        evaluator.call_function("greet", &args);
        assert_eq!(evaluator.get_logs(), &["Hello World"]);
    }

    #[test]
    fn test_merge_program_does_not_overwrite_existing() {
        let mut evaluator = FlowEvaluator::new();

        // Load a program with a function
        let host = FlowProgram {
            imports: vec![],
            globals: vec![],
            functions: vec![FunctionDef {
                name: "greet".to_string(),
                params: vec!["name".to_string()],
                body: vec![Stmt::Log(Expr::string("Original"), Span::default())],
                span: Span::default(),
            }],
            workflows: vec![],
            tests: vec![],
            span: Span::default(),
        };
        evaluator.load_program(&host);

        // Try to merge another program with the same function name
        let other = FlowProgram {
            imports: vec![],
            globals: vec![],
            functions: vec![FunctionDef {
                name: "greet".to_string(),
                params: vec!["name".to_string()],
                body: vec![Stmt::Log(Expr::string("Override"), Span::default())],
                span: Span::default(),
            }],
            workflows: vec![],
            tests: vec![],
            span: Span::default(),
        };
        evaluator.merge_program(&other);

        // The original function should still be used
        let args = vec![Value::String("World".to_string())];
        evaluator.call_function("greet", &args);
        assert_eq!(evaluator.get_logs(), &["Original"]);
    }

    // ---- Native function tests ----

    #[test]
    fn test_register_native_function_and_call() {
        let mut evaluator = FlowEvaluator::new();
        evaluator.register_native_function(
            "add",
            Box::new(|args| {
                let a = args[0].as_f64().unwrap_or(0.0);
                let b = args[1].as_f64().unwrap_or(0.0);
                serde_json::json!(a + b)
            }),
        );

        let args = vec![Value::Number(10.0), Value::Number(20.0)];
        let result = evaluator.call_function("add", &args);
        assert!(matches!(result, Value::Number(30.0)));
    }

    #[test]
    fn test_native_function_replaces_builtin() {
        let mut evaluator = FlowEvaluator::new();
        // Override the built-in `len` with a custom one
        evaluator.register_native_function(
            "len",
            Box::new(|_| serde_json::json!(42)),
        );

        let args = vec![Value::String("short".to_string())];
        let result = evaluator.call_function("len", &args);
        // Our custom `len` returns 42, not 5
        assert!(matches!(result, Value::Number(42.0)));
    }

    #[test]
    fn test_native_function_returns_json_types() {
        let mut evaluator = FlowEvaluator::new();
        evaluator.register_native_function(
            "get_config",
            Box::new(|_| serde_json::json!({"enabled": true, "count": 5})),
        );

        let result = evaluator.call_function("get_config", &[]);
        match result {
            Value::Object(map) => {
                assert_eq!(map.get("enabled"), Some(&Value::Bool(true)));
                assert_eq!(map.get("count"), Some(&Value::Number(5.0)));
            }
            _ => panic!("expected Object, got {:?}", result),
        }
    }

    #[test]
    fn test_set_native_functions_batch() {
        let mut evaluator = FlowEvaluator::new();
        let mut funcs = HashMap::new();
        funcs.insert(
            "double".to_string(),
            Box::new(|args: &[serde_json::Value]| {
                let n = args[0].as_f64().unwrap_or(0.0);
                serde_json::json!(n * 2.0)
            }) as NativeFunction,
        );
        funcs.insert(
            "triple".to_string(),
            Box::new(|args: &[serde_json::Value]| {
                let n = args[0].as_f64().unwrap_or(0.0);
                serde_json::json!(n * 3.0)
            }) as NativeFunction,
        );
        evaluator.set_native_functions(funcs);

        let args = vec![Value::Number(5.0)];
        assert!(matches!(evaluator.call_function("double", &args), Value::Number(10.0)));
        assert!(matches!(evaluator.call_function("triple", &args), Value::Number(15.0)));
    }

    // ---- Object getter tests ----

    #[test]
    fn test_register_object_getter_var() {
        let mut evaluator = FlowEvaluator::new();
        evaluator.register_object_getter(
            "config",
            Box::new(|path| match path {
                "" => Some(serde_json::json!({"base_url": "https://api.example.com"})),
                "base_url" => Some(serde_json::json!("https://api.example.com")),
                _ => None,
            }),
        );

        // `config` (root) should resolve via getter
        let vars = HashMap::new();
        let expr = Expr::var("config");
        let result = evaluator.eval_expr(&expr, &vars);
        match result {
            Value::Object(map) => {
                assert_eq!(
                    map.get("base_url"),
                    Some(&Value::String("https://api.example.com".to_string()))
                );
            }
            _ => panic!("expected Object for config root, got {:?}", result),
        }
    }

    #[test]
    fn test_register_object_getter_member() {
        let mut evaluator = FlowEvaluator::new();
        evaluator.register_object_getter(
            "config",
            Box::new(|path| match path {
                "base_url" => Some(serde_json::json!("https://api.example.com")),
                "timeout" => Some(serde_json::json!(30)),
                _ => None,
            }),
        );

        // `config.base_url` should resolve via getter
        let vars = HashMap::new();
        let expr = Expr::member(Expr::var("config"), "base_url");
        let result = evaluator.eval_expr(&expr, &vars);
        assert!(matches!(result, Value::String(s) if s == "https://api.example.com"));

        // `config.timeout` should return a number
        let expr2 = Expr::member(Expr::var("config"), "timeout");
        let result2 = evaluator.eval_expr(&expr2, &vars);
        assert!(matches!(result2, Value::Number(30.0)));

        // `config.nonexistent` should fall through to Null
        let expr3 = Expr::member(Expr::var("config"), "nonexistent");
        let result3 = evaluator.eval_expr(&expr3, &vars);
        assert!(matches!(result3, Value::Null));
    }

    #[test]
    fn test_object_getter_not_found_returns_null() {
        let mut evaluator = FlowEvaluator::new();
        // No getter registered for "unknown"
        let vars = HashMap::new();
        let expr = Expr::var("unknown");
        let result = evaluator.eval_expr(&expr, &vars);
        assert!(matches!(result, Value::Null));
    }

    #[test]
    fn test_native_function_called_from_expression() {
        let mut evaluator = FlowEvaluator::new();
        evaluator.register_native_function(
            "square",
            Box::new(|args| {
                let n = args[0].as_f64().unwrap_or(0.0);
                serde_json::json!(n * n)
            }),
        );

        // Simulate: var result = square(5)
        let vars = HashMap::new();
        let call_expr = Expr::call("square", vec![Expr::number(5.0)]);
        let result = evaluator.eval_expr(&call_expr, &vars);
        assert!(matches!(result, Value::Number(25.0)));
    }
}
