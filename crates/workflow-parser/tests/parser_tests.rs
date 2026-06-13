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
        let code = r#"
if (x > 10) {
  log("high")
}
"#;
        let stmts = FlowParser::parse_program(code).unwrap();
        assert_eq!(stmts.len(), 1);
        assert!(matches!(&stmts[0], Stmt::If { .. }));
    }

    #[test]
    fn test_parse_multiple() {
        let code = r#"
var x = 10
if (x > 5) {
  log("high")
}
"#;
        let stmts = FlowParser::parse_program(code).unwrap();
        assert_eq!(stmts.len(), 2);
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
