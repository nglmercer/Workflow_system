use regex::Regex;
use workflow_domain::{
    ComparisonOperator, Condition, ConditionGroup, LogicOperator, RuleCondition, TriggerContext,
    WorkflowError, WorkflowResult,
};

pub fn evaluate_condition(
    condition: &RuleCondition,
    context: &TriggerContext,
) -> WorkflowResult<bool> {
    match condition {
        RuleCondition::Single(cond) => evaluate_single(cond, context),
        RuleCondition::Group(group) => evaluate_group(group, context),
    }
}

fn evaluate_group(group: &ConditionGroup, context: &TriggerContext) -> WorkflowResult<bool> {
    match group.operator {
        LogicOperator::And => {
            for cond in &group.conditions {
                if !evaluate_condition(cond, context)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        LogicOperator::Or => {
            for cond in &group.conditions {
                if evaluate_condition(cond, context)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
    }
}

fn evaluate_single(condition: &Condition, context: &TriggerContext) -> WorkflowResult<bool> {
    let field_value = resolve_field(&condition.field, context)?;

    match condition.operator {
        ComparisonOperator::Eq => values_equal(&field_value, &condition.value),
        ComparisonOperator::Neq => Ok(!values_equal(&field_value, &condition.value)?),
        ComparisonOperator::Gt => compare_numeric(&field_value, &condition.value, |a, b| a > b),
        ComparisonOperator::Gte => compare_numeric(&field_value, &condition.value, |a, b| a >= b),
        ComparisonOperator::Lt => compare_numeric(&field_value, &condition.value, |a, b| a < b),
        ComparisonOperator::Lte => compare_numeric(&field_value, &condition.value, |a, b| a <= b),
        ComparisonOperator::In => {
            if let Some(arr) = condition.value.as_array() {
                for item in arr {
                    if values_equal(&field_value, item)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            } else {
                Err(WorkflowError::ConditionEvaluation(
                    "IN operator requires an array value".to_string(),
                ))
            }
        }
        ComparisonOperator::NotIn => {
            if let Some(arr) = condition.value.as_array() {
                for item in arr {
                    if values_equal(&field_value, item)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            } else {
                Err(WorkflowError::ConditionEvaluation(
                    "NOT_IN operator requires an array value".to_string(),
                ))
            }
        }
        ComparisonOperator::Contains => {
            let hay = field_value_as_string(&field_value)?;
            let needle = condition.value.as_str().unwrap_or("");
            Ok(hay.contains(needle))
        }
        ComparisonOperator::NotContains => {
            let hay = field_value_as_string(&field_value)?;
            let needle = condition.value.as_str().unwrap_or("");
            Ok(!hay.contains(needle))
        }
        ComparisonOperator::StartsWith => {
            let hay = field_value_as_string(&field_value)?;
            let prefix = condition.value.as_str().unwrap_or("");
            Ok(hay.starts_with(prefix))
        }
        ComparisonOperator::EndsWith => {
            let hay = field_value_as_string(&field_value)?;
            let suffix = condition.value.as_str().unwrap_or("");
            Ok(hay.ends_with(suffix))
        }
        ComparisonOperator::IsEmpty => match &field_value {
            serde_json::Value::String(s) => Ok(s.is_empty()),
            serde_json::Value::Array(a) => Ok(a.is_empty()),
            serde_json::Value::Object(o) => Ok(o.is_empty()),
            serde_json::Value::Null => Ok(true),
            _ => Ok(false),
        },
        ComparisonOperator::IsNull => Ok(field_value.is_null()),
        ComparisonOperator::IsNone => Ok(field_value.is_null()),
        ComparisonOperator::HasKey => {
            let key = condition.value.as_str().unwrap_or("");
            match &field_value {
                serde_json::Value::Object(map) => Ok(map.contains_key(key)),
                _ => Ok(false),
            }
        }
        ComparisonOperator::Matches => {
            let hay = field_value_as_string(&field_value)?;
            let pattern = condition.value.as_str().unwrap_or("");
            let re = Regex::new(pattern).map_err(|e| {
                WorkflowError::ConditionEvaluation(workflow_i18n::tf(
                    "engine.condition_invalid_regex",
                    &[("error", &e.to_string())],
                ))
            })?;
            Ok(re.is_match(&hay))
        }
        ComparisonOperator::Range => {
            let num = field_value_as_f64(&field_value)?;
            if let (Some(min), Some(max)) = (
                condition.value.get(0).and_then(|v| v.as_f64()),
                condition.value.get(1).and_then(|v| v.as_f64()),
            ) {
                Ok(num >= min && num <= max)
            } else {
                Err(WorkflowError::ConditionEvaluation(
                    "RANGE operator requires [min, max] array".to_string(),
                ))
            }
        }
        ComparisonOperator::Since
        | ComparisonOperator::After
        | ComparisonOperator::Before
        | ComparisonOperator::Until => {
            let field_ts = field_value_as_i64(&field_value)?;
            let value_ts = condition.value.as_i64().unwrap_or(0);
            match condition.operator {
                ComparisonOperator::Since | ComparisonOperator::After => Ok(field_ts >= value_ts),
                ComparisonOperator::Before | ComparisonOperator::Until => Ok(field_ts < value_ts),
                _ => unreachable!(),
            }
        }
    }
}

fn resolve_field(path: &str, context: &TriggerContext) -> WorkflowResult<serde_json::Value> {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = serde_json::to_value(context)
        .map_err(|e| WorkflowError::ConditionEvaluation(e.to_string()))?;

    for part in &parts {
        current = match &current {
            serde_json::Value::Object(map) => {
                map.get(*part).cloned().unwrap_or(serde_json::Value::Null)
            }
            serde_json::Value::Array(arr) => {
                if let Ok(idx) = part.parse::<usize>() {
                    arr.get(idx).cloned().unwrap_or(serde_json::Value::Null)
                } else {
                    serde_json::Value::Null
                }
            }
            _ => serde_json::Value::Null,
        };
    }

    Ok(current)
}

fn values_equal(a: &serde_json::Value, b: &serde_json::Value) -> WorkflowResult<bool> {
    Ok(a == b)
}

fn compare_numeric(
    a: &serde_json::Value,
    b: &serde_json::Value,
    cmp: impl Fn(f64, f64) -> bool,
) -> WorkflowResult<bool> {
    let a_num = field_value_as_f64(a)?;
    let b_num = b.as_f64().ok_or_else(|| {
        WorkflowError::ConditionEvaluation(workflow_i18n::t("engine.condition_right_not_number"))
    })?;
    Ok(cmp(a_num, b_num))
}

fn field_value_as_string(val: &serde_json::Value) -> WorkflowResult<String> {
    match val {
        serde_json::Value::String(s) => Ok(s.clone()),
        serde_json::Value::Null => Ok(String::new()),
        _ => Ok(val.to_string()),
    }
}

fn field_value_as_f64(val: &serde_json::Value) -> WorkflowResult<f64> {
    match val {
        serde_json::Value::Number(n) => Ok(n.as_f64().unwrap_or(0.0)),
        serde_json::Value::String(s) => s.parse::<f64>().map_err(|_| {
            WorkflowError::ConditionEvaluation(workflow_i18n::tf(
                "engine.condition_convert_to_number",
                &[("value", &s)],
            ))
        }),
        serde_json::Value::Bool(true) => Ok(1.0),
        serde_json::Value::Bool(false) => Ok(0.0),
        serde_json::Value::Null => Ok(0.0),
        _ => Err(WorkflowError::ConditionEvaluation(
            "Cannot convert value to number".to_string(),
        )),
    }
}

fn field_value_as_i64(val: &serde_json::Value) -> WorkflowResult<i64> {
    match val {
        serde_json::Value::Number(n) => Ok(n.as_i64().unwrap_or(0)),
        serde_json::Value::String(s) => s.parse::<i64>().map_err(|_| {
            WorkflowError::ConditionEvaluation(workflow_i18n::tf(
                "engine.condition_convert_to_integer",
                &[("value", &s)],
            ))
        }),
        _ => Err(WorkflowError::ConditionEvaluation(
            "Cannot convert value to integer".to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_context(data: serde_json::Value) -> TriggerContext {
        TriggerContext::new("TEST_EVENT", data)
    }

    #[test]
    fn test_eq_condition() {
        let ctx = make_context(json!({"user": {"plan": "premium"}}));
        let cond = Condition {
            field: "data.user.plan".to_string(),
            operator: ComparisonOperator::Eq,
            value: json!("premium"),
        };
        assert!(evaluate_single(&cond, &ctx).unwrap());
    }

    #[test]
    fn test_gt_condition() {
        let ctx = make_context(json!({"amount": 1500}));
        let cond = Condition {
            field: "data.amount".to_string(),
            operator: ComparisonOperator::Gt,
            value: json!(1000),
        };
        assert!(evaluate_single(&cond, &ctx).unwrap());
    }

    #[test]
    fn test_in_condition() {
        let ctx = make_context(json!({"role": "admin"}));
        let cond = Condition {
            field: "data.role".to_string(),
            operator: ComparisonOperator::In,
            value: json!(["admin", "moderator"]),
        };
        assert!(evaluate_single(&cond, &ctx).unwrap());
    }

    #[test]
    fn test_and_group() {
        let ctx = make_context(json!({"amount": 1500, "currency": "USD"}));
        let group = ConditionGroup {
            operator: LogicOperator::And,
            conditions: vec![
                RuleCondition::Single(Condition {
                    field: "data.amount".to_string(),
                    operator: ComparisonOperator::Gt,
                    value: json!(1000),
                }),
                RuleCondition::Single(Condition {
                    field: "data.currency".to_string(),
                    operator: ComparisonOperator::Eq,
                    value: json!("USD"),
                }),
            ],
        };
        assert!(evaluate_group(&group, &ctx).unwrap());
    }

    #[test]
    fn test_or_group() {
        let ctx = make_context(json!({"role": "guest"}));
        let group = ConditionGroup {
            operator: LogicOperator::Or,
            conditions: vec![
                RuleCondition::Single(Condition {
                    field: "data.role".to_string(),
                    operator: ComparisonOperator::Eq,
                    value: json!("admin"),
                }),
                RuleCondition::Single(Condition {
                    field: "data.role".to_string(),
                    operator: ComparisonOperator::Eq,
                    value: json!("guest"),
                }),
            ],
        };
        assert!(evaluate_group(&group, &ctx).unwrap());
    }

    #[test]
    fn test_matches_condition() {
        let ctx = make_context(json!({"email": "user@example.com"}));
        let cond = Condition {
            field: "data.email".to_string(),
            operator: ComparisonOperator::Matches,
            value: json!("^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\\.[a-zA-Z]{2,}$"),
        };
        assert!(evaluate_single(&cond, &ctx).unwrap());
    }

    #[test]
    fn test_contains_condition() {
        let ctx = make_context(json!({"tags": "important-urgent"}));
        let cond = Condition {
            field: "data.tags".to_string(),
            operator: ComparisonOperator::Contains,
            value: json!("urgent"),
        };
        assert!(evaluate_single(&cond, &ctx).unwrap());
    }

    #[test]
    fn test_is_empty_condition() {
        let ctx = make_context(json!({"name": ""}));
        let cond = Condition {
            field: "data.name".to_string(),
            operator: ComparisonOperator::IsEmpty,
            value: json!(null),
        };
        assert!(evaluate_single(&cond, &ctx).unwrap());
    }
}
