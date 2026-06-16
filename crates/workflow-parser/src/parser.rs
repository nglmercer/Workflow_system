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
            imports: Vec::new(),
            globals: Vec::new(),
            functions: Vec::new(),
            workflows: Vec::new(),
            tests: Vec::new(),
        };

        for pair in pairs {
            for inner in pair.into_inner() {
                match inner.as_rule() {
                    Rule::import_stmt => {
                        program.imports.push(parse_import(inner));
                    }
                    Rule::fn_def => {
                        program.functions.push(parse_fn_def(inner));
                    }
                    Rule::workflow_def => {
                        program.workflows.push(parse_workflow_def(inner));
                    }
                    Rule::test_def => {
                        program.tests.push(parse_test_def(inner));
                    }
                    Rule::comment => {
                        // Top-level comment — ignored.
                    }
                    _ => {
                        if let Some(Stmt::VarDecl { name, value }) = parse_stmt(inner) {
                            program.globals.push(GlobalVar {
                                name,
                                value: value.unwrap_or(Expr::Null),
                            });
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
            let text = inner.as_str().to_string();
            Some(Stmt::Expr(parse_expr_text(&text)))
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
            Rule::if_stmt => {
                // else if - wrap in a single-element vec
                let else_if_stmt = parse_if(inner);
                else_body = Some(vec![else_if_stmt]);
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
    let mut event = String::new();
    let mut params = Vec::new();

    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::IDENT => {
                event = inner.as_str().to_string();
            }
            Rule::destructure_params => {
                for param_pair in inner.into_inner() {
                    if param_pair.as_rule() == Rule::destructure_list {
                        params = parse_destructure_params(param_pair.as_str());
                    }
                }
            }
            _ => {}
        }
    }

    Stmt::On { event, params }
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

fn parse_import(pair: pest::iterators::Pair<Rule>) -> ImportStmt {
    // The grammar produces a single `import_stmt` whose head is
    // either `"@" ~ "import" ~ IDENT` (data-schema import) or
    // `"import" ~ IDENT` (regular module import), followed by
    // `"from" ~ import_source`.
    //
    // The two forms are now structurally identical: the only
    // difference is whether the source position starts with `@`.
    // The IDENT child is the binding name; it becomes a synthetic
    // scope binding once the LSP resolves the source.
    //
    // `import_source` is an indirection that holds either a STRING
    // (path or URL) or a `value_object` (inline JSON schema). We
    // descend into the first one we see.
    let mut name = String::new();
    let mut source: Option<ImportSource> = None;

    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::IDENT if name.is_empty() => {
                name = inner.as_str().to_string();
            }
            Rule::import_source => {
                for child in inner.into_inner() {
                    match child.as_rule() {
                        Rule::STRING => {
                            let s = child.as_str();
                            source = Some(ImportSource::Path(
                                s[1..s.len() - 1].to_string(),
                            ));
                        }
                        Rule::value_object => {
                            source =
                                Some(ImportSource::Inline(value_object_to_json(child)));
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    ImportStmt {
        name,
        source: source.unwrap_or(ImportSource::Path(String::new())),
    }
}

/// Convert a `value_object` pest pair into a `serde_json::Value` of
/// shape `Object`. The grammar guarantees the entries are
/// `value_object_entry` nodes, each containing an `IDENT` key and a
/// value that may be a `value_literal`, a `value_array`, or a nested
/// `value_object`. We dispatch on the value's rule to pick the right
/// conversion.
fn value_object_to_json(pair: pest::iterators::Pair<Rule>) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for inner in pair.into_inner() {
        if inner.as_rule() == Rule::value_object_entry {
            let mut key = String::new();
            let mut val = serde_json::Value::Null;
            for child in inner.into_inner() {
                match child.as_rule() {
                    Rule::IDENT => key = child.as_str().to_string(),
                    Rule::value_literal
                    | Rule::value_array
                    | Rule::value_object => val = value_literal_to_json(child),
                    _ => {}
                }
            }
            map.insert(key, val);
        }
    }
    serde_json::Value::Object(map)
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
            Rule::block => {
                let block_stmts = parse_block(inner);
                for stmt in block_stmts {
                    match &stmt {
                        Stmt::On {
                            event: evt,
                            params: p,
                        } => {
                            event = evt.clone();
                            params = p.clone();
                        }
                        _ => {
                            body.push(stmt);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    WorkflowDef {
        name,
        event,
        params,
        body,
    }
}

fn parse_test_def(pair: pest::iterators::Pair<Rule>) -> TestDef {
    let mut name = String::new();
    let mut on: Option<OnClause> = None;
    let mut expects: Vec<ExpectClause> = Vec::new();

    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::STRING => {
                let s = inner.as_str();
                name = s[1..s.len() - 1].to_string();
            }
            Rule::test_block => {
                for child in inner.into_inner() {
                    match child.as_rule() {
                        Rule::on_clause => on = Some(parse_on_clause(child)),
                        Rule::expect_clause => {
                            if let Some(exp) = parse_expect_clause(child) {
                                expects.push(exp);
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    TestDef {
        name,
        on: on.unwrap_or(OnClause {
            event: String::new(),
            data: serde_json::Value::Null,
        }),
        expects,
    }
}

fn parse_on_clause(pair: pest::iterators::Pair<Rule>) -> OnClause {
    let mut event = String::new();
    let mut data: Option<serde_json::Value> = None;
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::IDENT if event.is_empty() => {
                event = inner.as_str().to_string();
            }
            Rule::value_literal => {
                data = Some(value_literal_to_json(inner));
            }
            _ => {}
        }
    }
    OnClause {
        event,
        data: data.unwrap_or(serde_json::Value::Null),
    }
}

fn parse_expect_clause(pair: pest::iterators::Pair<Rule>) -> Option<ExpectClause> {
    // The discriminator (`"logs"`, `"emitted"`, `"return"`, `"var"`)
    // is a literal token in the grammar, so it doesn't appear as a
    // child pair. We sniff the raw source text to figure out which
    // alternative matched. The pair always starts with the literal
    // `expect`, so skip past that.
    let text = pair.as_str();
    let after_expect = text
        .trim_start()
        .strip_prefix("expect")
        .unwrap_or(text)
        .trim_start();
    let kind = if after_expect.starts_with("logs") {
        "logs"
    } else if after_expect.starts_with("emitted") {
        "emitted"
    } else if after_expect.starts_with("return") {
        "return"
    } else if after_expect.starts_with("var") {
        "var"
    } else {
        return None;
    };

    let mut value: Option<serde_json::Value> = None;
    let mut var_name: Option<String> = None;
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::IDENT => {
                if kind == "var" && var_name.is_none() {
                    var_name = Some(inner.as_str().to_string());
                }
            }
            Rule::value_literal | Rule::value_array | Rule::value_object => {
                value = Some(value_literal_to_json(inner));
            }
            _ => {}
        }
    }

    let json = value.unwrap_or(serde_json::Value::Null);
    match kind {
        "logs" => Some(ExpectClause::Logs(json_to_string_vec(&json))),
        "emitted" => Some(ExpectClause::Emitted(json_to_string_vec(&json))),
        "return" => Some(ExpectClause::Return(json)),
        "var" => Some(ExpectClause::Var {
            name: var_name.unwrap_or_default(),
            value: json,
        }),
        _ => None,
    }
}

fn json_to_string_vec(value: &serde_json::Value) -> Vec<String> {
    match value {
        serde_json::Value::Array(items) => items
            .iter()
            .map(|v| match v {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn value_literal_to_json(pair: pest::iterators::Pair<Rule>) -> serde_json::Value {
    // The grammar has a wrapper rule (`value_literal`) and two
    // structural alternatives (`value_array`, `value_object`) that
    // both live in the same alternative position. The wrapper is
    // always a single child of the alternative; the structural
    // rules contain their own children. Dispatch on the pair's own
    // rule first so we don't lose the structure.
    match pair.as_rule() {
        Rule::value_literal => {
            if let Some(inner) = pair.into_inner().next() {
                value_literal_to_json(inner)
            } else {
                serde_json::Value::Null
            }
        }
        Rule::value_array => {
            let mut items = Vec::new();
            for inner in pair.into_inner() {
                if matches!(
                    inner.as_rule(),
                    Rule::value_literal | Rule::value_array | Rule::value_object
                ) {
                    items.push(value_literal_to_json(inner));
                }
            }
            serde_json::Value::Array(items)
        }
        Rule::value_object => {
            let mut map = serde_json::Map::new();
            for inner in pair.into_inner() {
                if inner.as_rule() == Rule::value_object_entry {
                    let mut key = String::new();
                    let mut val = serde_json::Value::Null;
                    for child in inner.into_inner() {
                        match child.as_rule() {
                            Rule::IDENT => key = child.as_str().to_string(),
                            Rule::value_literal
                            | Rule::value_array
                            | Rule::value_object => val = value_literal_to_json(child),
                            _ => {}
                        }
                    }
                    map.insert(key, val);
                }
            }
            serde_json::Value::Object(map)
        }
        Rule::STRING => {
            let s = pair.as_str();
            serde_json::Value::String(s[1..s.len() - 1].to_string())
        }
        Rule::NUMBER => {
            let raw = pair.as_str();
            if let Ok(n) = raw.parse::<i64>() {
                serde_json::Value::from(n)
            } else if let Ok(n) = raw.parse::<f64>() {
                serde_json::json!(n)
            } else {
                serde_json::Value::Null
            }
        }
        Rule::bool_lit => serde_json::Value::Bool(pair.as_str() == "true"),
        Rule::NULL => serde_json::Value::Null,
        _ => serde_json::Value::Null,
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
    if let Some(stripped) = text.strip_prefix('!') {
        return Expr::UnaryOp {
            op: UnaryOp::Not,
            operand: Box::new(parse_expr_text(stripped)),
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

/// Best-effort source range for an expression. Used by lints to attach
/// a location to a diagnostic. The heuristic prefers identifier-level
/// anchors (so we never get fooled by repeated literals) and falls
/// back to a literal-text search. Returns `None` when no plausible
/// match exists in `source`.
///
/// Specifics:
/// - `Var(name)` and `Call { name, .. }` search for the identifier
///   preceded by a non-identifier byte (to avoid matching `foo` inside
///   `foobar`). For `Call`, we anchor on `name(` so we never hit a
///   local variable that happens to share the function's name.
/// - `Member { property, .. }` anchors on `.property`.
/// - String/number literals are searched verbatim. If the literal
///   appears multiple times we accept the *first* match (this is the
///   same compromise the previous heuristic made); lints that need
///   more precision should add parser-level spans.
/// - `BinaryOp`/`UnaryOp`/`Array`/`InterpolatedString` fall back to
///   the parenthesized text or the rendered expression.
pub fn find_expr_range(source: &str, expr: &Expr) -> Option<Span> {
    match expr {
        Expr::Var(name) => find_ident_range(source, name),
        Expr::Call { name, .. } => find_call_range(source, name),
        Expr::Member { property, .. } => find_member_range(source, property),
        Expr::String(s) => find_literal_range(source, &format!("\"{}\"", s), '"'),
        Expr::Number(n) => {
            // Match the rendered form. We try the integer form first
            // (matches what users type for round numbers) and fall
            // back to the float form.
            if n.fract() == 0.0 {
                find_literal_range(source, &format!("{}", *n as i64), '\0')
                    .or_else(|| find_literal_range(source, &format!("{}", n), '\0'))
            } else {
                find_literal_range(source, &format!("{}", n), '\0')
            }
        }
        Expr::Bool(b) => {
            let s = if *b { "true" } else { "false" };
            find_ident_range(source, s)
        }
        Expr::Null => find_ident_range(source, "null"),
        Expr::Array(_elems) => {
            // Find the first `[` and try to find a matching `]`. If
            // not, fall back to the first literal in the array.
            let text = expr_to_text(expr);
            find_balanced_range(source, &text, '[', ']')
        }
        Expr::BinaryOp { .. } | Expr::UnaryOp { .. } | Expr::InterpolatedString(_) => {
            let text = expr_to_text(expr);
            // No reliable structural anchor — fall back to substring
            // search of the rendered text. Accept first match.
            let needle = text.trim();
            if needle.is_empty() {
                return None;
            }
            source
                .find(needle)
                .map(|start| Span::new(start, start + needle.len()))
        }
    }
}

/// Find an identifier in `source` that is bounded on both sides by
/// non-identifier characters. Returns the first such occurrence, or
/// `None` if the identifier is not present as a free-standing word.
fn find_ident_range(source: &str, name: &str) -> Option<Span> {
    if name.is_empty() || !is_ident(name) {
        return None;
    }
    let bytes = source.as_bytes();
    let name_bytes = name.as_bytes();
    let mut i = 0;
    while i + name_bytes.len() <= bytes.len() {
        if &bytes[i..i + name_bytes.len()] == name_bytes {
            let before_ok = i == 0 || !is_ident_byte(bytes[i - 1]);
            let after_idx = i + name_bytes.len();
            let after_ok = after_idx == bytes.len() || !is_ident_byte(bytes[after_idx]);
            if before_ok && after_ok {
                return Some(Span::new(i, after_idx));
            }
        }
        i += 1;
    }
    None
}

/// Find a call site by anchoring on `name(`. This avoids matching a
/// local variable of the same name as a function.
fn find_call_range(source: &str, name: &str) -> Option<Span> {
    if name.is_empty() || !is_ident(name) {
        return None;
    }
    let needle = format!("{}(", name);
    let bytes = source.as_bytes();
    let nlen = name.len();
    let mut i = 0;
    while i + needle.len() <= bytes.len() {
        if &bytes[i..i + needle.len()] == needle.as_bytes() {
            let before_ok = i == 0 || !is_ident_byte(bytes[i - 1]);
            if before_ok {
                // The range covers the function name only — that's
                // the most useful anchor for diagnostic UI.
                return Some(Span::new(i, i + nlen));
            }
        }
        i += 1;
    }
    // Fall back to identifier-only search.
    find_ident_range(source, name)
}

/// Find a member-access site by anchoring on `.property`.
fn find_member_range(source: &str, property: &str) -> Option<Span> {
    if property.is_empty() || !is_ident(property) {
        return None;
    }
    let needle = format!(".{}", property);
    let bytes = source.as_bytes();
    let nlen = needle.len();
    let mut i = 0;
    while i + nlen <= bytes.len() {
        if &bytes[i..i + nlen] == needle.as_bytes() {
            return Some(Span::new(i, i + nlen));
        }
        i += 1;
    }
    None
}

/// Find a literal token like `"hello"` or `42`. The `quote` argument
/// is the optional leading quote character (so string searches can
/// avoid matching the content of other strings).
fn find_literal_range(source: &str, text: &str, quote: char) -> Option<Span> {
    if text.is_empty() {
        return None;
    }
    let bytes = source.as_bytes();
    let needle = text.as_bytes();
    let mut i = 0;
    while i + needle.len() <= bytes.len() {
        if &bytes[i..i + needle.len()] == needle {
            if quote != '\0' {
                // For strings, ensure the byte before is the opening
                // quote (or is a non-identifier byte to avoid matching
                // inside a larger string).
                if i > 0 && bytes[i - 1] != quote as u8 && is_ident_byte(bytes[i - 1]) {
                    i += 1;
                    continue;
                }
            } else {
                // For numbers, ensure the byte before and after are
                // not identifier-like (so `4` doesn't match inside `42`).
                if i > 0 && is_ident_byte(bytes[i - 1]) {
                    i += 1;
                    continue;
                }
                let after_idx = i + needle.len();
                if after_idx < bytes.len() && is_ident_byte(bytes[after_idx]) {
                    i += 1;
                    continue;
                }
            }
            return Some(Span::new(i, i + needle.len()));
        }
        i += 1;
    }
    None
}

/// Find a balanced `[...]` (or `(...)`) range in `source`. `text` is
/// the rendered expression we expect to find; we locate the first
/// occurrence of the open bracket and walk forward counting nesting.
fn find_balanced_range(source: &str, text: &str, open: char, close: char) -> Option<Span> {
    let bytes = source.as_bytes();
    let ob = open as u8;
    let cb = close as u8;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == ob {
            // Walk forward, ignoring anything inside strings.
            let mut depth = 1i32;
            let mut in_string = false;
            let mut escaped = false;
            let mut j = i + 1;
            while j < bytes.len() {
                let c = bytes[j];
                if in_string {
                    if escaped {
                        escaped = false;
                    } else if c == b'\\' {
                        escaped = true;
                    } else if c == b'"' {
                        in_string = false;
                    }
                    j += 1;
                    continue;
                }
                match c {
                    b'"' => in_string = true,
                    x if x == ob => depth += 1,
                    x if x == cb => {
                        depth -= 1;
                        if depth == 0 {
                            // If `text` is a substring of the source
                            // starting at `i`, accept the range. We
                            // do a best-effort check by comparing
                            // the first line of `text` to the source
                            // line starting at `i`.
                            return Some(Span::new(i, j + 1));
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
        }
        i += 1;
    }
    // Fall back: just find the rendered text.
    let needle = text.trim();
    if needle.is_empty() {
        return None;
    }
    source
        .find(needle)
        .map(|start| Span::new(start, start + needle.len()))
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn expr_to_text(expr: &Expr) -> String {
    match expr {
        Expr::String(s) => format!("\"{}\"", s),
        Expr::Number(n) => {
            if n.fract() == 0.0 {
                format!("{}", *n as i64)
            } else {
                format!("{}", n)
            }
        }
        Expr::Bool(b) => format!("{}", b),
        Expr::Null => "null".to_string(),
        Expr::Var(name) => name.clone(),
        Expr::Call { name, args } => {
            let arg_strs: Vec<String> = args.iter().map(expr_to_text).collect();
            format!("{}({})", name, arg_strs.join(", "))
        }
        Expr::Member { object, property } => {
            format!("{}.{}", expr_to_text(object), property)
        }
        Expr::BinaryOp { op, left, right } => {
            let op_str = match op {
                BinaryOp::Add => "+",
                BinaryOp::Sub => "-",
                BinaryOp::Mul => "*",
                BinaryOp::Div => "/",
                BinaryOp::Mod => "%",
                BinaryOp::Eq => "==",
                BinaryOp::Neq => "!=",
                BinaryOp::Lt => "<",
                BinaryOp::Gt => ">",
                BinaryOp::Lte => "<=",
                BinaryOp::Gte => ">=",
                BinaryOp::And => "&&",
                BinaryOp::Or => "||",
            };
            format!("{} {} {}", expr_to_text(left), op_str, expr_to_text(right))
        }
        Expr::UnaryOp { op, operand } => {
            let op_str = match op {
                UnaryOp::Not => "!",
                UnaryOp::Neg => "-",
            };
            format!("{}{}", op_str, expr_to_text(operand))
        }
        Expr::Array(elems) => {
            let elem_strs: Vec<String> = elems.iter().map(expr_to_text).collect();
            format!("[{}]", elem_strs.join(", "))
        }
        Expr::InterpolatedString(_) => "...".to_string(),
    }
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
            Stmt::On { event, params } => {
                assert_eq!(event, "TEST_EVENT");
                assert!(params.is_empty());
            }
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
        let code = r#"workflow "Nested Loops" {
  on NESTED_DATA ({users, meta})
  log("Users: " + users.length + ", Meta: " + meta.length)
}"#;
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
        let code = r#"workflow "Nested Loops" {
  on NESTED_DATA ({users, meta})
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

    #[test]
    fn test_parse_regular_import() {
        let program = FlowParser::parse_flow_program(r#"import utils from "./utils.flow""#)
            .unwrap();
        assert_eq!(program.imports.len(), 1);
        assert_eq!(program.imports[0].name, "utils");
        assert_eq!(
            program.imports[0].source,
            ImportSource::Path("./utils.flow".to_string())
        );
    }

    #[test]
    fn test_parse_data_import() {
        let program = FlowParser::parse_flow_program(r#"@import data from "./schema.json""#)
            .unwrap();
        assert_eq!(program.imports.len(), 1);
        assert_eq!(program.imports[0].name, "data");
        assert_eq!(
            program.imports[0].source,
            ImportSource::Path("./schema.json".to_string())
        );
    }

    #[test]
    fn test_parse_mixed_imports() {
        let code = r#"import utils from "./utils.flow"
@import data from "./schema.json""#;
        let program = FlowParser::parse_flow_program(code).unwrap();
        assert_eq!(program.imports.len(), 2);
        assert_eq!(program.imports[0].name, "utils");
        assert_eq!(
            program.imports[0].source,
            ImportSource::Path("./utils.flow".to_string())
        );
        assert_eq!(program.imports[1].name, "data");
        assert_eq!(
            program.imports[1].source,
            ImportSource::Path("./schema.json".to_string())
        );
    }

    #[test]
    fn test_parse_url_import() {
        let program = FlowParser::parse_flow_program(
            r#"@import data from "https://api.example.com/schemas/NESTED_DATA.json""#,
        )
        .unwrap();
        assert_eq!(program.imports.len(), 1);
        assert_eq!(program.imports[0].name, "data");
        assert_eq!(
            program.imports[0].source,
            ImportSource::Path(
                "https://api.example.com/schemas/NESTED_DATA.json".to_string()
            )
        );
    }

    #[test]
    fn test_parse_inline_schema_import() {
        let program = FlowParser::parse_flow_program(
            r#"@import data from { users: [], meta: { count: 0, source: "" } }"#,
        )
        .unwrap();
        assert_eq!(program.imports.len(), 1);
        assert_eq!(program.imports[0].name, "data");
        match &program.imports[0].source {
            ImportSource::Inline(value) => {
                let map = value.as_object().expect("object");
                assert!(map.contains_key("users"));
                assert!(map.contains_key("meta"));
                let users = map.get("users").unwrap();
                assert!(users.is_array(), "users should be an array, got {:?}", users);
                let meta = map.get("meta").unwrap().as_object().expect("meta is object");
                assert!(meta.contains_key("count"));
                assert!(meta.contains_key("source"));
            }
            other => panic!("expected Inline, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_named_module_import_with_inline_object() {
        // The regular `import <name> from ...` form is allowed to
        // point at an inline object too — the schema layer treats
        // both forms uniformly. The name is what binds the schema
        // to a workflow.
        let program = FlowParser::parse_flow_program(
            r#"import NESTED_DATA from { users: [], meta: [] }"#,
        )
        .unwrap();
        assert_eq!(program.imports.len(), 1);
        assert_eq!(program.imports[0].name, "NESTED_DATA");
        assert!(matches!(
            program.imports[0].source,
            ImportSource::Inline(_)
        ));
    }

    #[test]
    fn test_parse_named_data_import() {
        // `@import <name> from ...` is the same shape as
        // `import <name> from ...` — the `@` only marks the
        // import as a data-schema import. The name is no longer
        // hardcoded to `data`; users can declare one per event.
        let program = FlowParser::parse_flow_program(
            r#"@import USER_REGISTERED from { email: "", plan: "" }"#,
        )
        .unwrap();
        assert_eq!(program.imports.len(), 1);
        assert_eq!(program.imports[0].name, "USER_REGISTERED");
        assert!(matches!(
            program.imports[0].source,
            ImportSource::Inline(_)
        ));
    }

    #[test]
    fn test_parse_multiple_named_data_imports() {
        // The typical pattern: one `@import` per event, each with
        // its own name. The two names are independent bindings and
        // neither overwrites the other.
        let program = FlowParser::parse_flow_program(
            r#"@import USER_REGISTERED from { email: "", plan: "" }
@import BATCH_START from { items: [] }"#,
        )
        .unwrap();
        assert_eq!(program.imports.len(), 2);
        assert_eq!(program.imports[0].name, "USER_REGISTERED");
        assert_eq!(program.imports[1].name, "BATCH_START");
    }
}
