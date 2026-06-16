//! Type-annotation comments: `//@<type>` above a binding.
//!
//! These are the source of explicit type hints the inference engine can
//! use instead of falling back to `Any`. They are also the mechanism for
//! declaring function signatures, including parameter and return types.

use std::collections::HashMap;

use super::ty::Type;
use super::value::FunctionSig;

/// Pre-parsed set of type-annotation comments in a source file.
#[derive(Debug, Default, Clone)]
pub struct Annotations {
    /// `//@<type>` directly above a `var <name>` at the top level.
    pub globals: HashMap<String, Type>,
    /// `//@<type>` directly above a local `var <name> = ...` inside a
    /// function/workflow body.
    pub locals: HashMap<String, Type>,
    /// `//@{name: T, name: T, ...}` directly above a `fn <name>(...)` or
    /// `workflow "..."` block, optionally ending with `-> <ret>`.
    pub functions: HashMap<String, FunctionSig>,
    /// `//@<type>` lines inside a function body to annotate individual
    /// parameters. Key is `(function_name, param_name)`.
    pub param_types: HashMap<(String, String), Type>,
}

pub fn parse_annotations(source: &str) -> Annotations {
    let mut ann = Annotations::default();
    let lines: Vec<&str> = source.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix("//@") else {
            continue;
        };
        let body = rest.trim();
        // We need to know what's on the *next* code line to decide
        // whether this is a global, a local var, a function signature,
        // or a parameter annotation.
        let next = lines
            .iter()
            .skip(i + 1)
            .find(|l| !l.trim().is_empty() && !l.trim().starts_with("//@"));
        let Some(next_line) = next else { continue };
        let next_trim = next_line.trim();
        if let Some(param_spec) = body.strip_prefix("param ") {
            // `//@param name: type` — annotate a single parameter of
            // the *next* function. Checked first so the generic
            // `//@{...}` signature parser below doesn't swallow it.
            if let Some((name, ty)) = param_spec.split_once(':') {
                if let Some(next_line) = next {
                    if let Some(rest) = next_line.trim().strip_prefix("fn ") {
                        let fn_name = rest
                            .split(|c: char| !c.is_alphanumeric() && c != '_')
                            .next()
                            .unwrap_or("")
                            .to_string();
                        if !fn_name.is_empty() {
                            if let Ok(t) = parse_type(ty.trim()) {
                                ann.param_types
                                    .insert((fn_name, name.trim().to_string()), t);
                            }
                        }
                    }
                }
            }
        } else if let Some(rest) = next_trim.strip_prefix("fn ") {
            if let Some(sig) = parse_function_signature(body, rest) {
                ann.functions.insert(sig.name.clone(), sig);
            }
        } else if next_trim.starts_with("workflow ") {
            // Workflows have no parameter list to annotate, so `//@T` on
            // a workflow is not meaningful today — skip.
        } else if let Some(rest) = next_trim.strip_prefix("var ") {
            // `var name = ...` or `var name`
            let name = rest
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .next()
                .unwrap_or("")
                .to_string();
            if !name.is_empty() {
                if let Ok(t) = parse_type(body) {
                    ann.locals.insert(name.clone(), t.clone());
                    ann.globals.insert(name, t);
                }
            }
        }
    }
    ann
}

fn parse_function_signature(body: &str, fn_header: &str) -> Option<FunctionSig> {
    // fn_header is the part after `fn `, e.g. `summarize(user, count) { ...`.
    let name = fn_header
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .next()?
        .to_string();
    if name.is_empty() {
        return None;
    }
    // Body of the annotation can be:
    //   "{user:string, count:number}"             — params only, ret Any
    //   "{user:string} -> string"                — params + ret
    let body = body.trim();
    let (params_str, ret) = if let Some(idx) = body.find("->") {
        (&body[..idx], body[idx + 2..].trim())
    } else {
        (body, "any")
    };
    let params_str = params_str.trim();
    if !params_str.starts_with('{') || !params_str.ends_with('}') {
        return None;
    }
    let inner = &params_str[1..params_str.len() - 1];
    let mut param_types = Vec::new();
    let mut params = Vec::new();
    for entry in split_top_level_commas(inner) {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let (pname, pty) = entry.split_once(':')?;
        let pname = pname.trim().to_string();
        let pty = parse_type(pty.trim()).ok()?;
        params.push(pname);
        param_types.push(pty);
    }
    let ret = parse_type(ret).unwrap_or(Type::Any);
    Some(FunctionSig {
        name,
        params,
        param_types,
        ret,
        annotated: true,
    })
}

fn split_top_level_commas(s: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, ch) in s.char_indices() {
        match ch {
            '<' | '{' | '(' | '[' => depth += 1,
            '>' | '}' | ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(s[start..i].to_string());
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(s[start..].to_string());
    parts
}

fn parse_type(s: &str) -> Result<Type, String> {
    let s = s.trim();
    match s {
        "string" => Ok(Type::String),
        "number" => Ok(Type::Number),
        "bool" => Ok(Type::Bool),
        "null" => Ok(Type::Null),
        "any" => Ok(Type::Any),
        _ if s.ends_with("[]") => {
            let inner = parse_type(&s[..s.len() - 2])?;
            Ok(Type::Array(Box::new(inner)))
        }
        _ if s.starts_with('{') && s.ends_with('}') => {
            let inner = &s[1..s.len() - 1];
            let mut fields = Vec::new();
            for entry in split_top_level_commas(inner) {
                let (k, v) = entry
                    .split_once(':')
                    .ok_or_else(|| format!("expected `name: type` in {{...}}, got {}", entry))?;
                fields.push((k.trim().to_string(), parse_type(v.trim())?));
            }
            Ok(Type::Object(fields))
        }
        _ => Err(format!("unknown type: {}", s)),
    }
}
