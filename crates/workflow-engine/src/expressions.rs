use regex::Regex;
use workflow_domain::WorkflowResult;

use once_cell::sync::Lazy;

static EXPR_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\$\{([^}]+)\}").expect("Invalid expression regex"));

/// Evaluate expressions in a string, replacing ${path} with resolved values
pub fn evaluate_expressions(input: &str, context: &serde_json::Value) -> WorkflowResult<String> {
    let result = EXPR_REGEX.replace_all(input, |caps: &regex::Captures| {
        let expr = &caps[1];
        match resolve_expression(expr, context) {
            Ok(val) => val_to_string(&val),
            Err(_) => caps[0].to_string(), // Keep original on error
        }
    });

    Ok(result.to_string())
}

/// Resolve a dot-notation expression against a JSON context
pub fn resolve_expression(
    expr: &str,
    context: &serde_json::Value,
) -> WorkflowResult<serde_json::Value> {
    let parts: Vec<&str> = expr.split('.').collect();
    let mut current = context;

    for part in &parts {
        current = match current {
            serde_json::Value::Object(map) => map.get(*part).unwrap_or(&serde_json::Value::Null),
            serde_json::Value::Array(arr) => {
                if let Ok(idx) = part.parse::<usize>() {
                    arr.get(idx).unwrap_or(&serde_json::Value::Null)
                } else {
                    &serde_json::Value::Null
                }
            }
            _ => &serde_json::Value::Null,
        };
    }

    Ok(current.clone())
}

/// Convert a JSON value to string representation
fn val_to_string(val: &serde_json::Value) -> String {
    match val {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => String::new(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => val.to_string(),
    }
}

/// Evaluate a string field, resolving any expressions
pub fn evaluate_field(
    val: &serde_json::Value,
    context: &serde_json::Value,
) -> WorkflowResult<serde_json::Value> {
    match val {
        serde_json::Value::String(s) => {
            if s.contains("${") {
                let evaluated = evaluate_expressions(s, context)?;
                // Try to parse as JSON if it looks like one
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&evaluated) {
                    Ok(parsed)
                } else {
                    Ok(serde_json::Value::String(evaluated))
                }
            } else {
                Ok(val.clone())
            }
        }
        serde_json::Value::Object(map) => {
            let mut result = serde_json::Map::new();
            for (k, v) in map {
                result.insert(k.clone(), evaluate_field(v, context)?);
            }
            Ok(serde_json::Value::Object(result))
        }
        serde_json::Value::Array(arr) => {
            let mut result = Vec::new();
            for v in arr {
                result.push(evaluate_field(v, context)?);
            }
            Ok(serde_json::Value::Array(result))
        }
        _ => Ok(val.clone()),
    }
}

/// Build a context object with loop variables
pub fn build_context_with_vars(
    base_context: &serde_json::Value,
    vars: &[(String, serde_json::Value)],
) -> serde_json::Value {
    let mut context = base_context.clone();

    if let serde_json::Value::Object(map) = &mut context {
        let mut vars_obj = serde_json::Map::new();
        for (key, val) in vars {
            vars_obj.insert(key.clone(), val.clone());
        }
        map.insert("vars".to_string(), serde_json::Value::Object(vars_obj));
    }

    context
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_simple_expression() {
        let context = json!({"data": {"name": "John"}});
        let result = evaluate_expressions("Hello ${data.name}!", &context).unwrap();
        assert_eq!(result, "Hello John!");
    }

    #[test]
    fn test_nested_expression() {
        let context = json!({"data": {"user": {"id": 123}}});
        let result = evaluate_expressions("User: ${data.user.id}", &context).unwrap();
        assert_eq!(result, "User: 123");
    }

    #[test]
    fn test_multiple_expressions() {
        let context = json!({"data": {"first": "John", "last": "Doe"}});
        let result = evaluate_expressions("${data.first} ${data.last}", &context).unwrap();
        assert_eq!(result, "John Doe");
    }

    #[test]
    fn test_array_index() {
        let context = json!({"data": {"items": ["a", "b", "c"]}});
        let result = evaluate_expressions("${data.items.1}", &context).unwrap();
        assert_eq!(result, "b");
    }

    #[test]
    fn test_missing_field() {
        let context = json!({"data": {}});
        let result = evaluate_expressions("${data.missing}", &context).unwrap();
        assert_eq!(result, "");
    }
}
