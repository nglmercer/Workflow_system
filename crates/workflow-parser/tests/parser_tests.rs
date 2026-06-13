use workflow_domain::TriggerContext;
use workflow_parser::ast::*;
use workflow_parser::evaluator::Value;
use workflow_parser::{FlowCompiler, FlowEvaluator, FlowParser};

#[cfg(test)]
mod parser_tests {
    use super::*;

    #[test]
    fn test_parse_var() {
        let stmts = FlowParser::parse_program("var x = 42").unwrap();
        assert_eq!(stmts.len(), 1);
        match &stmts[0] {
            Stmt::VarDecl { name, .. } => assert_eq!(name, "x"),
            _ => panic!("Expected VarDecl"),
        }
    }

    #[test]
    fn test_parse_log() {
        let stmts = FlowParser::parse_program(r#"log("Hello")"#).unwrap();
        assert_eq!(stmts.len(), 1);
        assert!(matches!(&stmts[0], Stmt::Log(_)));
    }

    #[test]
    fn test_parse_if() {
        let code = "if (x > 10) { log(\"high\") }";
        let stmts = FlowParser::parse_program(code).unwrap();
        assert_eq!(stmts.len(), 1);
        assert!(matches!(&stmts[0], Stmt::If { .. }));
    }

    #[test]
    fn test_parse_if_multiline() {
        let code = "if (x > 10) {\n  log(\"high\")\n}";
        let stmts = FlowParser::parse_program(code).unwrap();
        assert_eq!(stmts.len(), 1);
        assert!(matches!(&stmts[0], Stmt::If { .. }));
    }

    #[test]
    fn test_parse_multiple() {
        let code = "var x = 10\nif (x > 5) { log(\"high\") }";
        let stmts = FlowParser::parse_program(code).unwrap();
        assert_eq!(stmts.len(), 2);
    }

    #[test]
    fn test_parse_foreach() {
        let code = "foreach (user in data.users) { log(user.name) }";
        let stmts = FlowParser::parse_program(code).unwrap();
        assert_eq!(stmts.len(), 1);
        match &stmts[0] {
            Stmt::Foreach { item_var, .. } => assert_eq!(item_var, "user"),
            _ => panic!("Expected Foreach"),
        }
    }

    #[test]
    fn test_parse_if_else() {
        let code = "if (x > 10) { log(\"high\") } else { log(\"low\") }";
        let stmts = FlowParser::parse_program(code).unwrap();
        assert_eq!(stmts.len(), 1);
        match &stmts[0] {
            Stmt::If { else_body, .. } => {
                assert!(else_body.is_some());
            }
            _ => panic!("Expected If"),
        }
    }

    #[test]
    fn test_parse_on_stmt() {
        let code = "on TEST_EVENT";
        let stmts = FlowParser::parse_program(code).unwrap();
        assert_eq!(stmts.len(), 1);
        match &stmts[0] {
            Stmt::On(evt) => assert_eq!(evt, "TEST_EVENT"),
            _ => panic!("Expected On"),
        }
    }

    #[test]
    fn test_parse_fn_def() {
        let code = "fn add(a, b) {\n  return a + b\n}";
        let program = FlowParser::parse_flow_program(code).unwrap();
        assert_eq!(program.functions.len(), 1);
        assert_eq!(program.functions[0].name, "add");
        assert_eq!(program.functions[0].params, vec!["a", "b"]);
    }

    #[test]
    fn test_parse_workflow_simple() {
        let code = "workflow \"Test\" {\n  on TEST_EVENT\n  log(\"hello\")\n}";
        let program = FlowParser::parse_flow_program(code).unwrap();
        assert_eq!(program.workflows.len(), 1);
        assert_eq!(program.workflows[0].name, "Test");
        assert_eq!(program.workflows[0].event, "TEST_EVENT");
        assert!(program.workflows[0].params.is_empty());
    }

    #[test]
    fn test_parse_workflow_with_destructure() {
        let code = "workflow \"Nested Loops\" ({users,meta}) {\n  on NESTED_DATA\n  log(\"Users: \" + users.length + \", Meta: \" + meta.length)\n}";
        let program = FlowParser::parse_flow_program(code).unwrap();
        assert_eq!(program.workflows.len(), 1);
        assert_eq!(program.workflows[0].name, "Nested Loops");
        assert_eq!(program.workflows[0].event, "NESTED_DATA");
        assert_eq!(program.workflows[0].params, vec!["users", "meta"]);
    }

    #[test]
    fn test_parse_nested_foreach() {
        let code = "workflow \"Nested Loops\" ({users,meta}) {\n  on NESTED_DATA\n  foreach (user in users) {\n    log(\"User: \" + user.name)\n    foreach (order in user.orders) {\n      log(\"  Order: \" + order.id)\n      if (order.total > 100) {\n        log(\"    High value order\")\n      }\n    }\n  }\n}";
        let program = FlowParser::parse_flow_program(code).unwrap();
        assert_eq!(program.workflows.len(), 1);
        assert_eq!(program.workflows[0].body.len(), 1);
        match &program.workflows[0].body[0] {
            Stmt::Foreach { item_var, body, .. } => {
                assert_eq!(item_var, "user");
                assert_eq!(body.len(), 2);
            }
            _ => panic!("Expected Foreach"),
        }
    }

    #[test]
    fn test_compile_simple() {
        let code = "workflow \"Test\" {\n  on TEST_EVENT\n  log(\"hello\")\n}";
        let program = FlowParser::parse_flow_program(code).unwrap();
        let rules = FlowCompiler::compile(&program).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].on, "TEST_EVENT");
    }

    #[test]
    fn test_compile_with_params() {
        let code = "workflow \"Nested Loops\" ({users,meta}) {\n  on NESTED_DATA\n  log(\"Users: \" + users.length)\n}";
        let program = FlowParser::parse_flow_program(code).unwrap();
        let rules = FlowCompiler::compile(&program).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].on, "NESTED_DATA");
    }

    #[test]
    fn test_evaluate_workflow() {
        let code = "workflow \"Test\" {\n  on TEST_EVENT\n  log(\"hello\")\n}";
        let program = FlowParser::parse_flow_program(code).unwrap();
        let mut evaluator = FlowEvaluator::new();
        evaluator.load_program(&program);

        let context = TriggerContext {
            event: "TEST_EVENT".to_string(),
            timestamp: 0,
            data: serde_json::json!({}),
            vars: None,
            id: None,
        };

        let logs = evaluator
            .execute_workflow(&program.workflows[0], &context)
            .unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0], "hello");
    }

    #[test]
    fn test_evaluate_with_destructure() {
        let code =
            "workflow \"Test\" ({users}) {\n  on TEST_EVENT\n  log(\"Count: \" + users.length)\n}";
        let program = FlowParser::parse_flow_program(code).unwrap();
        let mut evaluator = FlowEvaluator::new();
        evaluator.load_program(&program);

        let context = TriggerContext {
            event: "TEST_EVENT".to_string(),
            timestamp: 0,
            data: serde_json::json!({
                "users": [
                    {"name": "Alice"},
                    {"name": "Bob"}
                ]
            }),
            vars: None,
            id: None,
        };

        let logs = evaluator
            .execute_workflow(&program.workflows[0], &context)
            .unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0], "Count: 2");
    }

    #[test]
    fn test_evaluate_nested_foreach() {
        let code = "workflow \"Test\" ({users}) {\n  on TEST_EVENT\n  foreach (user in users) {\n    log(\"User: \" + user.name)\n  }\n}";
        let program = FlowParser::parse_flow_program(code).unwrap();
        let mut evaluator = FlowEvaluator::new();
        evaluator.load_program(&program);

        let context = TriggerContext {
            event: "TEST_EVENT".to_string(),
            timestamp: 0,
            data: serde_json::json!({
                "users": [
                    {"name": "Alice"},
                    {"name": "Bob"}
                ]
            }),
            vars: None,
            id: None,
        };

        let logs = evaluator
            .execute_workflow(&program.workflows[0], &context)
            .unwrap();
        assert_eq!(logs.len(), 2);
        assert_eq!(logs[0], "User: Alice");
        assert_eq!(logs[1], "User: Bob");
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
}
