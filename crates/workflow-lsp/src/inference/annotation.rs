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
        } else if !body.contains('{')
            && !body.contains('[')
            && !body.contains("->")
        {
            // `//@T1,T2,...` — positional per-parameter shortcut, one
            // type per parameter of the *next* function declaration.
            // The shortcut accepts a single type (e.g. `//@string`
            // for a 1-param function) as well as the multi-type
            // form. The full `//@{a:T, b:T} -> R` signature is
            // handled below for richer annotations.
            //
            // This is the form recommended for top-level utility
            // functions — see `examples/advanced.flow`.
            if let Some(rest) = next_trim.strip_prefix("fn ") {
                if let Some(sig) = parse_param_shortcut(body, rest) {
                    ann.functions.insert(sig.name.clone(), sig);
                    continue;
                }
            }
        }
        if let Some(rest) = next_trim.strip_prefix("fn ") {
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

/// Parse a positional per-parameter shortcut: `//@T1,T2,T3` directly
/// above a `fn name(a, b, c) { ... }` declaration. The number of types
/// must equal the number of parameters. The return type defaults to
/// `Any`. The form is intentionally compact — recommended for
/// top-level utility functions where the user wants to type
/// `//@string,string` once instead of
/// `//@{a:string, b:string} -> any`.
///
/// Each type token may be `@`-prefixed (treating `@` as a decorator
/// marker) so both `//@string,string` and `//@string,@string` work.
///
/// Example:
///
/// ```flow
/// //@string,string
/// fn formatCurrency(amount, currency) {
///   return currency + " " + amount
/// }
/// ```
fn parse_param_shortcut(body: &str, fn_header: &str) -> Option<FunctionSig> {
    // Parse the function header to discover the param names. This way
    // we don't have to trust that the annotation's arity matches the
    // function's arity — a mismatch is a parse error rather than a
    // silent type confusion.
    let (name, params) = parse_function_header(fn_header)?;
    // Parse the comma-separated types. Allow trailing whitespace and
    // a leading `@` on each token (decorator style).
    let types: Vec<Type> = body
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.strip_prefix('@').unwrap_or(s).trim())
        .filter(|s| !s.is_empty())
        .map(parse_type)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    if types.len() != params.len() {
        // Mismatch — fall through to inference rather than fail loudly.
        return None;
    }
    Some(FunctionSig {
        name,
        params,
        param_types: types,
        ret: Type::Any,
        annotated: true,
    })
}

/// Parse the leading `fn name(a, b, c) { ...` header into `(name, params)`.
/// Returns `None` if the header is malformed.
fn parse_function_header(fn_header: &str) -> Option<(String, Vec<String>)> {
    let name_end = fn_header
        .find(|c: char| c == '(' || c.is_whitespace())
        .unwrap_or(fn_header.len());
    let name = fn_header[..name_end].trim().to_string();
    if name.is_empty() || !is_ident(&name) {
        return None;
    }
    let open = fn_header.find('(')?;
    let rel_close = fn_header[open..].find(')')?;
    let close = open + rel_close;
    let params_str = &fn_header[open + 1..close];
    let params: Vec<String> = params_str
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect();
    Some((name, params))
}

fn is_ident(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
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
