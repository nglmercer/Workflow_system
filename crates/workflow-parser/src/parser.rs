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
                if let Some(stmt) = parse_stmt(inner) {
                    stmts.push(stmt);
                }
            }
        }
        Ok(stmts)
    }

    pub fn parse_flow_program(input: &str) -> Result<FlowProgram, String> {
        let pairs =
            FlowParser::parse(Rule::program, input).map_err(|e| format!("Parse error: {}", e))?;

        let mut program = FlowProgram {
            globals: Vec::new(),
            functions: Vec::new(),
            workflows: Vec::new(),
        };

        for pair in pairs {
            for inner in pair.into_inner() {
                match inner.as_rule() {
                    Rule::fn_def => {
                        program.functions.push(parse_fn_def(inner));
                    }
                    Rule::workflow_def => {
                        program.workflows.push(parse_workflow_def(inner));
                    }
                    _ => {
                        if let Some(stmt) = parse_stmt(inner) {
                            match stmt {
                                Stmt::VarDecl { name, value } => {
                                    program.globals.push(GlobalVar {
                                        name,
                                        value: value.unwrap_or(Expr::Null),
                                    });
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }

        Ok(program)
    }
}

fn parse_stmt(pair: pest::iterators::Pair<Rule>) -> Option<Stmt> {
    let inner = if pair.as_rule() == Rule::stmt {
        pair.into_inner().next()?
    } else {
        pair
    };

    match inner.as_rule() {
        Rule::var_decl => Some(parse_var_decl(inner)),
        Rule::return_stmt => Some(parse_return(inner)),
        Rule::if_stmt => Some(parse_if(inner)),
        Rule::log_stmt => Some(parse_log(inner)),
        Rule::foreach_stmt => Some(parse_foreach(inner)),
        Rule::on_stmt => Some(parse_on(inner)),
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

fn parse_return(pair: pest::iterators::Pair<Rule>) -> Stmt {
    let value = pair
        .into_inner()
        .find(|p| p.as_rule() == Rule::expr)
        .map(parse_expr);
    Stmt::Return { value }
}

fn parse_if(pair: pest::iterators::Pair<Rule>) -> Stmt {
    let mut condition = None;
    let mut then_body = Vec::new();
    let mut else_body = None;

    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::expr if condition.is_none() => {
                condition = Some(parse_expr(inner));
            }
            Rule::block if then_body.is_empty() => {
                then_body = parse_block(inner);
            }
            Rule::block => {
                else_body = Some(parse_block(inner));
            }
            _ => {}
        }
    }

    Stmt::If {
        condition: condition.unwrap_or(Expr::Bool(true)),
        then_body,
        else_body,
    }
}

fn parse_log(pair: pest::iterators::Pair<Rule>) -> Stmt {
    let expr = pair
        .into_inner()
        .next()
        .map(parse_expr)
        .unwrap_or(Expr::Null);
    Stmt::Log(expr)
}

fn parse_foreach(pair: pest::iterators::Pair<Rule>) -> Stmt {
    let mut item_var = None;
    let mut iterable = None;
    let mut body = Vec::new();

    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::IDENT if item_var.is_none() => {
                item_var = Some(inner.as_str().to_string());
            }
            Rule::expr => {
                iterable = Some(parse_expr(inner));
            }
            Rule::block => {
                body = parse_block(inner);
            }
            _ => {}
        }
    }

    Stmt::Foreach {
        item_var: item_var.unwrap_or_default(),
        iterable: iterable.unwrap_or(Expr::Null),
        body,
    }
}

fn parse_on(pair: pest::iterators::Pair<Rule>) -> Stmt {
    Stmt::On(
        pair.into_inner()
            .next()
            .map(|p| p.as_str().to_string())
            .unwrap_or_default(),
    )
}

fn parse_block(pair: pest::iterators::Pair<Rule>) -> Vec<Stmt> {
    let mut stmts = Vec::new();
    for p in pair.into_inner() {
        if let Some(stmt) = parse_stmt(p) {
            stmts.push(stmt);
        }
    }
    stmts
}

fn parse_fn_def(pair: pest::iterators::Pair<Rule>) -> FunctionDef {
    let mut name = String::new();
    let mut params = Vec::new();
    let mut body = Vec::new();

    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::IDENT if name.is_empty() => {
                name = inner.as_str().to_string();
            }
            Rule::IDENT => {
                params.push(inner.as_str().to_string());
            }
            Rule::block => {
                body = parse_block(inner);
            }
            _ => {}
        }
    }

    FunctionDef { name, params, body }
}

fn parse_workflow_def(pair: pest::iterators::Pair<Rule>) -> WorkflowDef {
    let mut name = String::new();
    let mut event = String::new();
    let mut params = Vec::new();
    let mut body = Vec::new();

    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::STRING => {
                let s = inner.as_str();
                name = s[1..s.len() - 1].to_string();
            }
            Rule::destructure_list => {
                params = parse_destructure_params(inner.as_str());
            }
            Rule::block => {
                let block_stmts = parse_block(inner);
                for stmt in block_stmts {
                    match &stmt {
                        Stmt::On(evt) => {
                            event = evt.clone();
                        }
                        _ => {
                            body.push(stmt);
                        }
                    }
                }
            }
            _ => {
                let text = inner.as_str().trim();
                if text.starts_with("({")
                    || (text.contains(',') && !text.contains('"') && !text.contains('{'))
                {
                    params = parse_destructure_params(text);
                }
            }
        }
    }

    WorkflowDef {
        name,
        event,
        params,
        body,
    }
}

fn parse_destructure_params(text: &str) -> Vec<String> {
    let mut trimmed = text.trim();
    if let Some(stripped) = trimmed
        .strip_prefix("({")
        .and_then(|s| s.strip_suffix("})"))
    {
        trimmed = stripped;
    }

    trimmed
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn parse_expr(pair: pest::iterators::Pair<Rule>) -> Expr {
    parse_expr_text(pair.as_str())
}

fn parse_expr_text(text: &str) -> Expr {
    let text = text.trim();
    if text.is_empty() {
        return Expr::Null;
    }
    if let Some(index) = find_top_level_op(text, "||") {
        return parse_binary_text(BinaryOp::Or, &text[..index], &text[index + 2..]);
    }
    if let Some(index) = find_top_level_op(text, "&&") {
        return parse_binary_text(BinaryOp::And, &text[..index], &text[index + 2..]);
    }
    for op in ["==", "!=", ">=", "<=", ">", "<"] {
        if let Some(index) = find_top_level_op(text, op) {
            let binary_op = match op {
                "==" => BinaryOp::Eq,
                "!=" => BinaryOp::Neq,
                ">=" => BinaryOp::Gte,
                "<=" => BinaryOp::Lte,
                ">" => BinaryOp::Gt,
                _ => BinaryOp::Lt,
            };
            return parse_binary_text(binary_op, &text[..index], &text[index + op.len()..]);
        }
    }
    for op in ['+', '-'] {
        if let Some(index) = find_top_level_op(text, &op.to_string()) {
            if index == 0 {
                continue;
            }
            let binary_op = if op == '+' {
                BinaryOp::Add
            } else {
                BinaryOp::Sub
            };
            return parse_binary_text(binary_op, &text[..index], &text[index + 1..]);
        }
    }
    for op in ['*', '/', '%'] {
        if let Some(index) = find_top_level_op(text, &op.to_string()) {
            let binary_op = match op {
                '*' => BinaryOp::Mul,
                '/' => BinaryOp::Div,
                _ => BinaryOp::Mod,
            };
            return parse_binary_text(binary_op, &text[..index], &text[index + 1..]);
        }
    }
    if text.starts_with('!') {
        return Expr::UnaryOp {
            op: UnaryOp::Not,
            operand: Box::new(parse_expr_text(&text[1..])),
        };
    }
    if text.starts_with('-') && text.len() > 1 {
        return Expr::UnaryOp {
            op: UnaryOp::Neg,
            operand: Box::new(parse_expr_text(&text[1..])),
        };
    }
    if text.starts_with('(') && text.ends_with(')') {
        return parse_expr_text(&text[1..text.len() - 1]);
    }
    parse_atom_text(text).unwrap_or(Expr::Null)
}

fn parse_binary_text(op: BinaryOp, left: &str, right: &str) -> Expr {
    Expr::binary(op, parse_expr_text(left), parse_expr_text(right))
}

fn parse_atom_text(text: &str) -> Option<Expr> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if text.starts_with('"') {
        return Some(Expr::String(parse_string_literal(text)));
    }
    if text == "true" {
        return Some(Expr::Bool(true));
    }
    if text == "false" {
        return Some(Expr::Bool(false));
    }
    if text == "null" {
        return Some(Expr::Null);
    }
    if text.starts_with('[') && text.ends_with(']') {
        return Some(parse_array_text(&text[1..text.len() - 1]));
    }
    if text.starts_with('-') || text.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        return Some(Expr::Number(text.parse().unwrap_or(0.0)));
    }
    if let Some(index) = text.find('(') {
        if text.ends_with(')') {
            let name = text[..index].trim().to_string();
            let args = split_top_level(&text[index + 1..text.len() - 1], ',')
                .into_iter()
                .filter(|arg| !arg.trim().is_empty())
                .map(|arg| parse_expr_text(arg.trim()))
                .collect();
            return Some(Expr::call(name, args));
        }
    }
    if let Some(index) = text.find('.') {
        let object = parse_atom_text(&text[..index])?;
        return Some(Expr::member(object, text[index + 1..].trim()));
    }
    if is_ident(text) {
        return Some(Expr::Var(text.to_string()));
    }
    None
}

fn parse_array_text(text: &str) -> Expr {
    let elems = split_top_level(text, ',')
        .into_iter()
        .filter(|elem| !elem.trim().is_empty())
        .map(|elem| parse_expr_text(elem.trim()))
        .collect();
    Expr::Array(elems)
}

fn parse_string_literal(text: &str) -> String {
    let mut result = String::new();
    let mut chars = text[1..].chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                if let Some(next) = chars.next() {
                    match next {
                        'n' => result.push('\n'),
                        't' => result.push('\t'),
                        '"' => result.push('"'),
                        '\\' => result.push('\\'),
                        other => result.push(other),
                    }
                }
            }
            '"' => break,
            other => result.push(other),
        }
    }
    result
}

fn split_top_level(text: &str, delimiter: char) -> Vec<String> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;

    for (index, ch) in text.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            _ if ch == delimiter && depth == 0 => {
                parts.push(text[start..index].to_string());
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(text[start..].to_string());
    parts
}

fn find_top_level_op(text: &str, op: &str) -> Option<usize> {
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    let bytes = text.as_bytes();
    let mut index = 0;

    while index < bytes.len() {
        let ch = bytes[index] as char;
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            index += 1;
            continue;
        }
        match ch {
            '"' => in_string = true,
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            _ if depth == 0 && text[index..].starts_with(op) => return Some(index),
            _ => {}
        }
        index += 1;
    }
    None
}

fn is_ident(text: &str) -> bool {
    let mut chars = text.chars();
    matches!(chars.next(), Some(ch) if ch.is_ascii_alphabetic() || ch == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn parse_call(pair: pest::iterators::Pair<Rule>) -> Expr {
    let mut inner = pair.into_inner();
    let name = inner.next().unwrap().as_str().to_string();
    let args = inner
        .filter(|p| p.as_rule() == Rule::expr)
        .map(parse_expr)
        .collect();

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
        let stmts = FlowParser::parse_program("if (x > 10) { log(\"high\") }").unwrap();
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
        let code = r#"fn add(a, b) {
  return a + b
}"#;
        let program = FlowParser::parse_flow_program(code).unwrap();
        assert_eq!(program.functions.len(), 1);
        assert_eq!(program.functions[0].name, "add");
        assert_eq!(program.functions[0].params, vec!["a", "b"]);
    }

    #[test]
    fn test_parse_workflow_simple() {
        let code = r#"workflow "Test" {
  on TEST_EVENT
  log("hello")
}"#;
        let program = FlowParser::parse_flow_program(code).unwrap();
        assert_eq!(program.workflows.len(), 1);
        assert_eq!(program.workflows[0].name, "Test");
        assert_eq!(program.workflows[0].event, "TEST_EVENT");
        assert!(program.workflows[0].params.is_empty());
    }

    #[test]
    fn test_parse_workflow_with_destructure() {
        let code = r#"workflow "Nested Loops" ({users,meta}) {
  on NESTED_DATA
  log("Users: " + users.length + ", Meta: " + meta.length)
}"#;
        let program = FlowParser::parse_flow_program(code).unwrap();
        assert_eq!(program.workflows.len(), 1);
        assert_eq!(program.workflows[0].name, "Nested Loops");
        assert_eq!(program.workflows[0].event, "NESTED_DATA");
        assert_eq!(program.workflows[0].params, vec!["users", "meta"]);
    }

    #[test]
    fn test_parse_nested_foreach() {
        let code = r#"workflow "Nested Loops" ({users,meta}) {
  on NESTED_DATA
  foreach (user in users) {
    log("User: " + user.name)
    foreach (order in user.orders) {
      log("  Order: " + order.id)
      if (order.total > 100) {
        log("    High value order")
      }
    }
  }
}"#;
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
}
