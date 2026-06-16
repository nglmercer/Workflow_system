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
            Stmt::On { event, params } => {
                assert_eq!(event, "TEST_EVENT");
                assert!(params.is_empty());
            }
            _ => panic!("Expected On"),
        }
    }

    #[test]
    fn test_parse_on_with_params() {
        let code = "on TEST_EVENT ({users, meta})";
        let stmts = FlowParser::parse_program(code).unwrap();
        assert_eq!(stmts.len(), 1);
        match &stmts[0] {
            Stmt::On { event, params } => {
                assert_eq!(event, "TEST_EVENT");
                assert_eq!(params, &vec!["users".to_string(), "meta".to_string()]);
            }
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
        let code = "workflow \"Nested Loops\" {\n  on NESTED_DATA ({users, meta})\n  log(\"Users: \" + users.length + \", Meta: \" + meta.length)\n}";
        let program = FlowParser::parse_flow_program(code).unwrap();
        assert_eq!(program.workflows.len(), 1);
        assert_eq!(program.workflows[0].name, "Nested Loops");
        assert_eq!(program.workflows[0].event, "NESTED_DATA");
        assert_eq!(
            program.workflows[0].params,
            vec!["users".to_string(), "meta".to_string()]
        );
    }

    #[test]
    fn test_parse_nested_foreach() {
        let code = "workflow \"Nested Loops\" {\n  on NESTED_DATA ({users, meta})\n  foreach (user in users) {\n    log(\"User: \" + user.name)\n    foreach (order in user.orders) {\n      log(\"  Order: \" + order.id)\n      if (order.total > 100) {\n        log(\"    High value order\")\n      }\n    }\n  }\n}";
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
        let code = "workflow \"Nested Loops\" {\n  on NESTED_DATA ({users, meta})\n  log(\"Users: \" + users.length)\n}";
        let program = FlowParser::parse_flow_program(code).unwrap();
        let rules = FlowCompiler::compile(&program).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].on, "NESTED_DATA");
    }

    #[test]
    fn test_parse_full_nested_example() {
        let code = "fn formatCurrency(amount, currency) {\n  return currency + \" \" + amount\n}\n\nworkflow \"Nested Loops\" {\n  on NESTED_DATA ({users, meta})\n  log(\"Users: \" + users.length + \", Meta: \" + meta.length)\n  foreach (user in users) {\n    log(\"User: \" + user.name)\n    foreach (order in user.orders) {\n      log(\"  Order: \" + order.id)\n      if (order.total > 100) {\n        log(\"    High value order\")\n      }\n    }\n  }\n}";
        let program = FlowParser::parse_flow_program(code).unwrap();
        assert_eq!(program.functions.len(), 1);
        assert_eq!(program.functions[0].name, "formatCurrency");
        assert_eq!(program.workflows.len(), 1);
        assert_eq!(program.workflows[0].name, "Nested Loops");
        assert_eq!(
            program.workflows[0].params,
            vec!["users".to_string(), "meta".to_string()]
        );
        assert_eq!(program.workflows[0].body.len(), 2);
    }

    #[test]
    fn test_evaluate_with_destructure() {
        let code =
            "workflow \"Test\" {\n  on TEST_EVENT ({users})\n  log(\"Count: \" + users.length)\n}";
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
        let code = "workflow \"Test\" {\n  on TEST_EVENT ({users})\n  foreach (user in users) {\n    log(\"User: \" + user.name)\n  }\n}";
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

    #[test]
    fn test_evaluate_user_defined_function() {
        let code = r#"fn double(x) {
  return x * 2
}

fn greet(name) {
  log("Hello, " + name + "!")
}

workflow "Test" {
  on CALCULATE
  var result = double(data.value)
  log("Doubled: " + result)
  greet(data.name)
}"#;
        let program = FlowParser::parse_flow_program(code).unwrap();
        assert_eq!(program.functions.len(), 2);
        assert_eq!(program.workflows.len(), 1);

        let mut evaluator = FlowEvaluator::new();
        evaluator.load_program(&program);

        let context = TriggerContext {
            event: "CALCULATE".to_string(),
            timestamp: 0,
            data: serde_json::json!({"value": 21, "name": "World"}),
            vars: None,
            id: None,
        };

        let logs = evaluator
            .execute_workflow(&program.workflows[0], &context)
            .unwrap();
        assert_eq!(logs, vec!["Doubled: 42", "Hello, World!"]);
    }

    #[test]
    fn test_parse_if_else_if() {
        let code = r#"workflow "Test" {
  on TEST_EVENT
  if (data.amount > 1000) {
    log("high")
  } else if (data.amount > 500) {
    log("medium")
  } else {
    log("low")
  }
}"#;
        let program = FlowParser::parse_flow_program(code).unwrap();
        assert_eq!(program.workflows.len(), 1);

        let mut evaluator = FlowEvaluator::new();
        evaluator.load_program(&program);

        // Test high branch
        let context = TriggerContext {
            event: "TEST_EVENT".to_string(),
            timestamp: 0,
            data: serde_json::json!({"amount": 1500}),
            vars: None,
            id: None,
        };
        let logs = evaluator
            .execute_workflow(&program.workflows[0], &context)
            .unwrap();
        assert_eq!(logs, vec!["high"]);

        // Test medium branch
        let context = TriggerContext {
            event: "TEST_EVENT".to_string(),
            timestamp: 0,
            data: serde_json::json!({"amount": 750}),
            vars: None,
            id: None,
        };
        let logs = evaluator
            .execute_workflow(&program.workflows[0], &context)
            .unwrap();
        assert_eq!(logs, vec!["medium"]);

        // Test low branch
        let context = TriggerContext {
            event: "TEST_EVENT".to_string(),
            timestamp: 0,
            data: serde_json::json!({"amount": 200}),
            vars: None,
            id: None,
        };
        let logs = evaluator
            .execute_workflow(&program.workflows[0], &context)
            .unwrap();
        assert_eq!(logs, vec!["low"]);
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

#[cfg(test)]
mod test_def_tests {
    use super::*;

    #[test]
    fn test_parse_test_def_simple() {
        let code = "test \"Premium user gets greeting\" {\n  on USER_REGISTERED with { name: \"Ada\", plan: \"premium\" }\n  expect logs [\"Hello Ada!\"]\n  expect emitted []\n  expect return null\n  expect var greeting == \"Hello Ada!\"\n}\n";
        let program = FlowParser::parse_flow_program(code).unwrap();
        assert_eq!(program.tests.len(), 1);
        let t = &program.tests[0];
        assert_eq!(t.name, "Premium user gets greeting");
        assert_eq!(t.on.event, "USER_REGISTERED");
        assert_eq!(t.on.data["name"], serde_json::json!("Ada"));
        assert_eq!(t.on.data["plan"], serde_json::json!("premium"));
        assert_eq!(t.expects.len(), 4);
        assert!(matches!(&t.expects[0], ExpectClause::Logs(v) if v == &vec!["Hello Ada!".to_string()]));
        assert!(matches!(&t.expects[1], ExpectClause::Emitted(v) if v.is_empty()));
        assert!(matches!(&t.expects[2], ExpectClause::Return(serde_json::Value::Null)));
        assert!(matches!(&t.expects[3], ExpectClause::Var { name, value } if name == "greeting" && *value == serde_json::json!("Hello Ada!")));
    }

    #[test]
    fn test_parse_test_def_with_all_assertions() {
        let code = "test \"Many asserts\" {\n  on CLICK with { count: 3, label: \"ok\" }\n  expect logs [\"a\", \"b\", \"c\"]\n  expect emitted [\"X\", \"Y\"]\n  expect return 42\n  expect var total == 7\n}\n";
        let program = FlowParser::parse_flow_program(code).unwrap();
        assert_eq!(program.tests.len(), 1);
        let t = &program.tests[0];
        assert_eq!(t.expects.len(), 4);
        assert!(matches!(&t.expects[0], ExpectClause::Logs(v) if v.len() == 3));
        assert!(matches!(&t.expects[1], ExpectClause::Emitted(v) if v == &vec!["X".to_string(), "Y".to_string()]));
        assert!(matches!(&t.expects[2], ExpectClause::Return(v) if v.as_i64() == Some(42)));
        assert!(matches!(&t.expects[3], ExpectClause::Var { name, value } if name == "total" && value.as_i64() == Some(7)));
    }

    #[test]
    fn test_parse_program_mixes_tests_and_workflows() {
        let code = "workflow \"Greet\" {\n  on USER_REGISTERED\n  log(\"Hello\")\n}\n\ntest \"Logs hello\" {\n  on USER_REGISTERED\n  expect logs [\"Hello\"]\n}\n";
        let program = FlowParser::parse_flow_program(code).unwrap();
        assert_eq!(program.workflows.len(), 1);
        assert_eq!(program.tests.len(), 1);
        assert_eq!(program.workflows[0].name, "Greet");
        assert_eq!(program.tests[0].name, "Logs hello");
    }
}
