use pest::Parser;
use pest_derive::Parser;

use crate::ast::*;

#[derive(Parser)]
#[grammar = "flow.pest"]
pub struct FlowParser;

impl FlowParser {
    pub fn parse_program(input: &str) -> Result<Vec<Stmt>, String> {
        let pairs =
            FlowParser::parse(Rule::program, input).map_err(|e| format!("Parse error: {}", e))?;

        let mut stmts = Vec::new();
        for pair in pairs {
            for inner in pair.into_inner() {
                if inner.as_rule() == Rule::stmt {
                    if let Some(stmt) = parse_stmt(inner) {
                        stmts.push(stmt);
                    }
                }
            }
        }
        Ok(stmts)
    }
}

fn parse_stmt(pair: pest::iterators::Pair<Rule>) -> Option<Stmt> {
    let inner = pair.into_inner().next()?;
    match inner.as_rule() {
        Rule::var_decl => Some(parse_var_decl(inner)),
        Rule::if_stmt => Some(parse_if(inner)),
        Rule::log_stmt => Some(parse_log(inner)),
        Rule::foreach_stmt => Some(parse_foreach(inner)),
        Rule::expr_stmt => {
            let expr_pair = inner.into_inner().next()?;
            Some(Stmt::Expr(parse_call(expr_pair)))
        }
        _ => None,
    }
}

fn parse_var_decl(pair: pest::iterators::Pair<Rule>) -> Stmt {
    let mut name = None;
    let mut value = None;
    for inner in pair.into_inner() {
        if matches!(inner.as_rule(), Rule::IDENT) && name.is_none() {
            name = Some(inner.as_str().to_string());
        } else if matches!(inner.as_rule(), Rule::expr) {
            value = Some(parse_expr(inner));
        }
    }
    Stmt::VarDecl {
        name: name.unwrap_or_default(),
        value,
    }
}

fn parse_if(pair: pest::iterators::Pair<Rule>) -> Stmt {
    let mut condition = None;
    let mut then_body = Vec::new();

    for inner in pair.into_inner() {
        if matches!(inner.as_rule(), Rule::expr) && condition.is_none() {
            condition = Some(parse_expr(inner));
        } else if matches!(inner.as_rule(), Rule::block) {
            then_body = parse_block(inner);
        }
    }

    Stmt::If {
        condition: condition.unwrap_or(Expr::Bool(true)),
        then_body,
        else_body: None,
    }
}

fn parse_log(pair: pest::iterators::Pair<Rule>) -> Stmt {
    let expr = pair
        .into_inner()
        .next()
        .map(|p| parse_expr(p))
        .unwrap_or(Expr::Null);
    Stmt::Log(expr)
}

fn parse_foreach(pair: pest::iterators::Pair<Rule>) -> Stmt {
    let mut item_var = None;
    let mut iterable = None;
    let mut body = Vec::new();

    for inner in pair.into_inner() {
        if matches!(inner.as_rule(), Rule::IDENT) && item_var.is_none() {
            item_var = Some(inner.as_str().to_string());
        } else if matches!(inner.as_rule(), Rule::expr) {
            iterable = Some(parse_expr(inner));
        } else if matches!(inner.as_rule(), Rule::block) {
            body = parse_block(inner);
        }
    }

    Stmt::Foreach {
        item_var: item_var.unwrap_or_default(),
        iterable: iterable.unwrap_or(Expr::Null),
        body,
    }
}

fn parse_block(pair: pest::iterators::Pair<Rule>) -> Vec<Stmt> {
    let mut stmts = Vec::new();
    for p in pair.into_inner() {
        if p.as_rule() == Rule::stmt {
            if let Some(stmt) = parse_stmt(p) {
                stmts.push(stmt);
            }
        }
    }
    stmts
}

fn parse_expr(pair: pest::iterators::Pair<Rule>) -> Expr {
    parse_additive(pair)
}

fn parse_additive(pair: pest::iterators::Pair<Rule>) -> Expr {
    let mut children: Vec<_> = pair.into_inner().collect();
    let mut result = parse_multiplicative(children.remove(0));

    while !children.is_empty() {
        let op_str = children.remove(0).as_str();
        let right = parse_multiplicative(children.remove(0));
        result = Expr::binary(
            match op_str {
                "+" => BinaryOp::Add,
                _ => BinaryOp::Sub,
            },
            result,
            right,
        );
    }
    result
}

fn parse_multiplicative(pair: pest::iterators::Pair<Rule>) -> Expr {
    let mut children: Vec<_> = pair.into_inner().collect();
    let mut result = parse_unary(children.remove(0));

    while !children.is_empty() {
        let op_str = children.remove(0).as_str();
        let right = parse_unary(children.remove(0));
        result = Expr::binary(
            match op_str {
                "*" => BinaryOp::Mul,
                _ => BinaryOp::Div,
            },
            result,
            right,
        );
    }
    result
}

fn parse_unary(pair: pest::iterators::Pair<Rule>) -> Expr {
    let inner = pair.into_inner().next().unwrap();
    parse_primary(inner)
}

fn parse_primary(pair: pest::iterators::Pair<Rule>) -> Expr {
    let inner = pair.into_inner().next().unwrap();
    match inner.as_rule() {
        Rule::literal => parse_literal(inner),
        Rule::var_ref => Expr::Var(inner.as_str().to_string()),
        Rule::call => parse_call(inner),
        Rule::group => parse_expr(inner.into_inner().next().unwrap()),
        _ => Expr::Null,
    }
}

fn parse_literal(pair: pest::iterators::Pair<Rule>) -> Expr {
    let inner = pair.into_inner().next().unwrap();
    match inner.as_rule() {
        Rule::STRING => {
            let s = inner.as_str();
            Expr::String(s[1..s.len() - 1].to_string())
        }
        Rule::NUMBER => {
            let n: f64 = inner.as_str().parse().unwrap_or(0.0);
            Expr::Number(n)
        }
        Rule::bool_lit => Expr::Bool(inner.as_str() == "true"),
        Rule::NULL => Expr::Null,
        _ => Expr::Null,
    }
}

fn parse_call(pair: pest::iterators::Pair<Rule>) -> Expr {
    let mut inner = pair.into_inner();
    let name = inner.next().unwrap().as_str().to_string();
    let mut args = Vec::new();

    for p in inner {
        if p.as_rule() == Rule::expr {
            args.push(parse_expr(p));
        }
    }

    Expr::call(name, args)
}

#[cfg(test)]
mod tests {
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
