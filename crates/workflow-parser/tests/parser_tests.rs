use workflow_parser::ast::*;
use workflow_parser::evaluator::Value;
use workflow_parser::{FlowCompiler, FlowEvaluator, FlowParser};

#[cfg(test)]
mod parser_tests {
    use super::*;

    #[test]
    fn test_parse_global_var() {
        let code = r#"var x = 42"#;
        let program = FlowParser::parse_program(code).unwrap();
        assert_eq!(program.globals.len(), 1);
        assert_eq!(program.globals[0].name, "x");
    }

    #[test]
    fn test_parse_global_var_string() {
        let code = r#"var name = "hello""#;
        let program = FlowParser::parse_program(code).unwrap();
        assert_eq!(program.globals.len(), 1);
        assert_eq!(program.globals[0].name, "name");
    }

    #[test]
    fn test_parse_global_var_bool() {
        let code = r#"var flag = true"#;
        let program = FlowParser::parse_program(code).unwrap();
        assert_eq!(program.globals.len(), 1);
    }

    #[test]
    fn test_parse_function() {
        let code = r#"
fn greet(name) {
  log("Hello " + name)
}
"#;
        let program = FlowParser::parse_program(code).unwrap();
        assert_eq!(program.functions.len(), 1);
        assert_eq!(program.functions[0].name, "greet");
        assert_eq!(program.functions[0].params, vec!["name"]);
    }

    #[test]
    fn test_parse_function_multiple_params() {
        let code = r#"
fn add(a, b) {
  return a + b
}
"#;
        let program = FlowParser::parse_program(code).unwrap();
        assert_eq!(program.functions.len(), 1);
        assert_eq!(program.functions[0].params, vec!["a", "b"]);
    }

    #[test]
    fn test_parse_workflow() {
        let code = r#"
workflow "Test Workflow" {
  on TEST_EVENT
  log("Hello")
}
"#;
        let program = FlowParser::parse_program(code).unwrap();
        assert_eq!(program.workflows.len(), 1);
        assert_eq!(program.workflows[0].name, "Test Workflow");
        assert_eq!(program.workflows[0].event, "TEST_EVENT");
    }

    #[test]
    fn test_parse_if_else() {
        let code = r#"
workflow "Conditional" {
  on TEST
  if (data.value > 10) {
    log("High")
  } else {
    log("Low")
  }
}
"#;
        let program = FlowParser::parse_program(code).unwrap();
        assert_eq!(program.workflows.len(), 1);
    }

    #[test]
    fn test_parse_nested_if() {
        let code = r#"
workflow "Nested" {
  on TEST
  if (data.a > 1) {
    if (data.b > 2) {
      log("Both high")
    }
  }
}
"#;
        let program = FlowParser::parse_program(code).unwrap();
        assert_eq!(program.workflows.len(), 1);
    }

    #[test]
    fn test_parse_foreach() {
        let code = r#"
workflow "Loop" {
  on TEST
  foreach (item in data.items) {
    log("Item: " + item.name)
  }
}
"#;
        let program = FlowParser::parse_program(code).unwrap();
        assert_eq!(program.workflows.len(), 1);
    }

    #[test]
    fn test_parse_multiple_workflows() {
        let code = r#"
workflow "First" {
  on EVENT1
  log("First")
}

workflow "Second" {
  on EVENT2
  log("Second")
}
"#;
        let program = FlowParser::parse_program(code).unwrap();
        assert_eq!(program.workflows.len(), 2);
    }

    #[test]
    fn test_parse_comments() {
        let code = r#"
// This is a comment
var x = 10
// Another comment
workflow "Test" {
  on EVENT
  log("Hello")
}
"#;
        let program = FlowParser::parse_program(code).unwrap();
        assert_eq!(program.globals.len(), 1);
        assert_eq!(program.workflows.len(), 1);
    }

    #[test]
    fn test_parse_arithmetic() {
        let code = r#"
workflow "Math" {
  on TEST
  var x = 10 + 20 * 3
  log("Result: " + x)
}
"#;
        let program = FlowParser::parse_program(code).unwrap();
        assert_eq!(program.workflows.len(), 1);
    }

    #[test]
    fn test_parse_comparison() {
        let code = r#"
workflow "Compare" {
  on TEST
  if (data.x >= 10 && data.y <= 20) {
    log("In range")
  }
}
"#;
        let program = FlowParser::parse_program(code).unwrap();
        assert_eq!(program.workflows.len(), 1);
    }

    #[test]
    fn test_parse_function_call() {
        let code = r#"
fn helper() {
  log("helper called")
}

workflow "Main" {
  on TEST
  helper()
}
"#;
        let program = FlowParser::parse_program(code).unwrap();
        assert_eq!(program.functions.len(), 1);
        assert_eq!(program.workflows.len(), 1);
    }

    #[test]
    fn test_parse_array() {
        let code = r#"
workflow "Array" {
  on TEST
  var items = [1, 2, 3]
  log("Items: " + items.length)
}
"#;
        let program = FlowParser::parse_program(code).unwrap();
        assert_eq!(program.workflows.len(), 1);
    }

    #[test]
    fn test_parse_member_access() {
        let code = r#"
workflow "Member" {
  on TEST
  var name = data.user.name
  log("Name: " + name)
}
"#;
        let program = FlowParser::parse_program(code).unwrap();
        assert_eq!(program.workflows.len(), 1);
    }
}

#[cfg(test)]
mod evaluator_tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_eval_number() {
        let mut evaluator = FlowEvaluator::new();
        let vars = HashMap::new();
        let result = evaluator.eval_expr(&Expr::number(42.0), &vars);
        assert!(matches!(result, Value::Number(42.0)));
    }

    #[test]
    fn test_eval_string() {
        let mut evaluator = FlowEvaluator::new();
        let vars = HashMap::new();
        let result = evaluator.eval_expr(&Expr::string("hello"), &vars);
        assert!(matches!(result, Value::String(s) if s == "hello"));
    }

    #[test]
    fn test_eval_bool() {
        let mut evaluator = FlowEvaluator::new();
        let vars = HashMap::new();
        let result = evaluator.eval_expr(&Expr::Bool(true), &vars);
        assert!(matches!(result, Value::Bool(true)));
    }

    #[test]
    fn test_eval_variable() {
        let mut evaluator = FlowEvaluator::new();
        let mut vars = HashMap::new();
        vars.insert("x".to_string(), Value::Number(10.0));
        let result = evaluator.eval_expr(&Expr::var("x"), &vars);
        assert!(matches!(result, Value::Number(10.0)));
    }

    #[test]
    fn test_eval_arithmetic_add() {
        let mut evaluator = FlowEvaluator::new();
        let vars = HashMap::new();
        let expr = Expr::binary(BinaryOp::Add, Expr::number(2.0), Expr::number(3.0));
        let result = evaluator.eval_expr(&expr, &vars);
        assert!(matches!(result, Value::Number(5.0)));
    }

    #[test]
    fn test_eval_arithmetic_mul() {
        let mut evaluator = FlowEvaluator::new();
        let vars = HashMap::new();
        let expr = Expr::binary(BinaryOp::Mul, Expr::number(4.0), Expr::number(5.0));
        let result = evaluator.eval_expr(&expr, &vars);
        assert!(matches!(result, Value::Number(20.0)));
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
    fn test_eval_comparison_eq() {
        let mut evaluator = FlowEvaluator::new();
        let vars = HashMap::new();
        let expr = Expr::binary(BinaryOp::Eq, Expr::number(10.0), Expr::number(10.0));
        let result = evaluator.eval_expr(&expr, &vars);
        assert!(matches!(result, Value::Bool(true)));
    }

    #[test]
    fn test_eval_comparison_gt() {
        let mut evaluator = FlowEvaluator::new();
        let vars = HashMap::new();
        let expr = Expr::binary(BinaryOp::Gt, Expr::number(10.0), Expr::number(5.0));
        let result = evaluator.eval_expr(&expr, &vars);
        assert!(matches!(result, Value::Bool(true)));
    }

    #[test]
    fn test_eval_logical_and() {
        let mut evaluator = FlowEvaluator::new();
        let vars = HashMap::new();
        let expr = Expr::binary(BinaryOp::And, Expr::Bool(true), Expr::Bool(false));
        let result = evaluator.eval_expr(&expr, &vars);
        assert!(matches!(result, Value::Bool(false)));
    }

    #[test]
    fn test_eval_logical_or() {
        let mut evaluator = FlowEvaluator::new();
        let vars = HashMap::new();
        let expr = Expr::binary(BinaryOp::Or, Expr::Bool(false), Expr::Bool(true));
        let result = evaluator.eval_expr(&expr, &vars);
        assert!(matches!(result, Value::Bool(true)));
    }

    #[test]
    fn test_eval_not() {
        let mut evaluator = FlowEvaluator::new();
        let vars = HashMap::new();
        let expr = Expr::UnaryOp {
            op: UnaryOp::Not,
            operand: Box::new(Expr::Bool(true)),
        };
        let result = evaluator.eval_expr(&expr, &vars);
        assert!(matches!(result, Value::Bool(false)));
    }

    #[test]
    fn test_eval_neg() {
        let mut evaluator = FlowEvaluator::new();
        let vars = HashMap::new();
        let expr = Expr::UnaryOp {
            op: UnaryOp::Neg,
            operand: Box::new(Expr::number(5.0)),
        };
        let result = evaluator.eval_expr(&expr, &vars);
        assert!(matches!(result, Value::Number(-5.0)));
    }

    #[test]
    fn test_eval_member_access() {
        let mut evaluator = FlowEvaluator::new();
        let mut vars = HashMap::new();
        let mut obj = HashMap::new();
        obj.insert("x".to_string(), Value::Number(42.0));
        vars.insert("data".to_string(), Value::Object(obj));

        let expr = Expr::member(Expr::var("data"), "x");
        let result = evaluator.eval_expr(&expr, &vars);
        assert!(matches!(result, Value::Number(42.0)));
    }

    #[test]
    fn test_eval_function_call() {
        let mut evaluator = FlowEvaluator::new();
        let vars = HashMap::new();
        let expr = Expr::call("log", vec![Expr::string("test")]);
        let result = evaluator.eval_expr(&expr, &vars);
        assert!(matches!(result, Value::Null));
    }

    #[test]
    fn test_eval_array() {
        let mut evaluator = FlowEvaluator::new();
        let vars = HashMap::new();
        let expr = Expr::Array(vec![
            Expr::number(1.0),
            Expr::number(2.0),
            Expr::number(3.0),
        ]);
        let result = evaluator.eval_expr(&expr, &vars);
        assert!(matches!(result, Value::Array(_)));
    }

    #[test]
    fn test_eval_workflow() {
        let mut evaluator = FlowEvaluator::new();
        let program = FlowParser::parse_program(
            r#"
workflow "Test" {
  on TEST
  log("Hello World")
}
"#,
        )
        .unwrap();

        evaluator.load_program(&program);

        let context = workflow_domain::TriggerContext::new("TEST", serde_json::json!({}));
        let logs = evaluator
            .execute_workflow(&program.workflows[0], &context)
            .unwrap();

        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0], "Hello World");
    }

    #[test]
    fn test_eval_workflow_with_condition() {
        let mut evaluator = FlowEvaluator::new();
        let program = FlowParser::parse_program(
            r#"
workflow "Test" {
  on TEST
  if (data.value > 10) {
    log("High")
  } else {
    log("Low")
  }
}
"#,
        )
        .unwrap();

        evaluator.load_program(&program);

        let context =
            workflow_domain::TriggerContext::new("TEST", serde_json::json!({"value": 15}));
        let logs = evaluator
            .execute_workflow(&program.workflows[0], &context)
            .unwrap();

        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0], "High");
    }

    #[test]
    fn test_eval_workflow_with_var() {
        let mut evaluator = FlowEvaluator::new();
        let program = FlowParser::parse_program(
            r#"
workflow "Test" {
  on TEST
  var x = data.value * 2
  log("Result: " + x)
}
"#,
        )
        .unwrap();

        evaluator.load_program(&program);

        let context = workflow_domain::TriggerContext::new("TEST", serde_json::json!({"value": 5}));
        let logs = evaluator
            .execute_workflow(&program.workflows[0], &context)
            .unwrap();

        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0], "Result: 10");
    }
}

#[cfg(test)]
mod compiler_tests {
    use super::*;

    #[test]
    fn test_compile_simple_workflow() {
        let program = FlowParser::parse_program(
            r#"
workflow "Test" {
  on TEST_EVENT
  log("Hello")
}
"#,
        )
        .unwrap();

        let rules = FlowCompiler::compile(&program).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].on, "TEST_EVENT");
        assert_eq!(rules[0].metadata.id, "test");
    }

    #[test]
    fn test_compile_workflow_with_var() {
        let program = FlowParser::parse_program(
            r#"
workflow "Test" {
  on TEST
  var x = 10
}
"#,
        )
        .unwrap();

        let rules = FlowCompiler::compile(&program).unwrap();
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn test_compile_multiple_workflows() {
        let program = FlowParser::parse_program(
            r#"
workflow "First" {
  on EVENT1
  log("First")
}

workflow "Second" {
  on EVENT2
  log("Second")
}
"#,
        )
        .unwrap();

        let rules = FlowCompiler::compile(&program).unwrap();
        assert_eq!(rules.len(), 2);
    }
}
