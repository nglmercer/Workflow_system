use crate::ast::*;
use workflow_domain::*;

/// Compiler that converts .flow AST to workflow rules
pub struct FlowCompiler;

impl FlowCompiler {
    pub fn compile(program: &FlowProgram) -> WorkflowResult<Vec<TriggerRule>> {
        let mut rules = Vec::new();

        for workflow in &program.workflows {
            let rule = Self::compile_workflow(workflow, &program.functions)?;
            rules.push(rule);
        }

        Ok(rules)
    }

    fn compile_workflow(
        workflow: &WorkflowDef,
        functions: &[FunctionDef],
    ) -> WorkflowResult<TriggerRule> {
        let actions = Self::compile_stmts(&workflow.body, functions)?;

        Ok(TriggerRule {
            metadata: RuleMetadata {
                id: slugify(&workflow.name),
                name: Some(workflow.name.clone()),
                description: None,
                priority: None,
                enabled: None,
                cooldown: None,
                tags: None,
            },
            on: workflow.event.clone(),
            condition: None,
            r#do: if actions.len() == 1 {
                ActionOrGroup::Single(actions.into_iter().next().unwrap())
            } else {
                ActionOrGroup::Multiple(actions)
            },
        })
    }

    fn compile_stmts(stmts: &[Stmt], _functions: &[FunctionDef]) -> WorkflowResult<Vec<Action>> {
        let mut actions = Vec::new();

        for stmt in stmts {
            match stmt {
                Stmt::VarDecl { name, value } => {
                    actions.push(Action {
                        action_type: "set_var".to_string(),
                        params: Some({
                            let mut m = std::collections::HashMap::new();
                            m.insert("key".to_string(), serde_json::json!(name));
                            if let Some(val) = value {
                                m.insert("value".to_string(), Self::compile_expr_to_json(val));
                            }
                            m
                        }),
                        delay: None,
                        probability: None,
                        retry: None,
                        foreach: None,
                        r#while: None,
                        repeat: None,
                    });
                }
                Stmt::If {
                    condition: _,
                    then_body,
                    else_body,
                } => {
                    // Compile if as a conditional action group
                    let then_actions = Self::compile_stmts(then_body, _functions)?;
                    let else_actions = else_body
                        .as_ref()
                        .map(|s| Self::compile_stmts(s, _functions))
                        .transpose()?
                        .unwrap_or_default();

                    // For now, just add then_actions (simplified)
                    actions.extend(then_actions);
                    actions.extend(else_actions);
                }
                Stmt::Log(expr) => {
                    actions.push(Action {
                        action_type: "log_message".to_string(),
                        params: Some({
                            let mut m = std::collections::HashMap::new();
                            m.insert("message".to_string(), Self::compile_expr_to_json(expr));
                            m.insert("level".to_string(), serde_json::json!("info"));
                            m
                        }),
                        delay: None,
                        probability: None,
                        retry: None,
                        foreach: None,
                        r#while: None,
                        repeat: None,
                    });
                }
                Stmt::Return { .. } => {
                    // Return is not used in workflow context
                }
                Stmt::Expr(Expr::Call { name, args }) => {
                    // Handle function calls
                    actions.push(Action {
                        action_type: name.clone(),
                        params: Some(
                            args.iter()
                                .enumerate()
                                .map(|(i, arg)| {
                                    (format!("arg{}", i), Self::compile_expr_to_json(arg))
                                })
                                .collect(),
                        ),
                        delay: None,
                        probability: None,
                        retry: None,
                        foreach: None,
                        r#while: None,
                        repeat: None,
                    });
                }
                Stmt::Expr(_) => {
                    // Other expressions are ignored
                }
                Stmt::Foreach {
                    item_var,
                    iterable,
                    body,
                } => {
                    let inner_actions = Self::compile_stmts(body, _functions)?;
                    actions.push(Action {
                        action_type: "noop".to_string(),
                        params: None,
                        delay: None,
                        probability: None,
                        retry: None,
                        foreach: Some(ForeachConfig {
                            field: Self::compile_expr_to_field_path(iterable),
                            item_var: item_var.clone(),
                            index_var: None,
                            actions: inner_actions,
                            parallel: None,
                        }),
                        r#while: None,
                        repeat: None,
                    });
                }
            }
        }

        Ok(actions)
    }

    fn compile_expr_to_json(expr: &Expr) -> serde_json::Value {
        match expr {
            Expr::String(s) => serde_json::json!(s),
            Expr::Number(n) => serde_json::json!(n),
            Expr::Bool(b) => serde_json::json!(b),
            Expr::Null => serde_json::Value::Null,
            Expr::Var(name) => serde_json::json!(format!("${{{}}}", name)),
            Expr::Member { object, property } => {
                let obj_str = Self::compile_expr_to_field_path(object);
                serde_json::json!(format!("${{{}.{}}}", obj_str, property))
            }
            Expr::BinaryOp { op, left, right } => {
                let l = Self::compile_expr_to_json(left);
                let r = Self::compile_expr_to_json(right);
                serde_json::json!({
                    "op": format!("{:?}", op),
                    "left": l,
                    "right": r
                })
            }
            Expr::Call { name, args } => {
                let arg_vals: Vec<serde_json::Value> =
                    args.iter().map(Self::compile_expr_to_json).collect();
                serde_json::json!({
                    "function": name,
                    "args": arg_vals
                })
            }
            Expr::Array(elems) => {
                let vals: Vec<serde_json::Value> =
                    elems.iter().map(Self::compile_expr_to_json).collect();
                serde_json::json!(vals)
            }
            Expr::UnaryOp { op, operand } => {
                let val = Self::compile_expr_to_json(operand);
                serde_json::json!({
                    "op": format!("{:?}", op),
                    "operand": val
                })
            }
            Expr::InterpolatedString(parts) => {
                let mut result = String::new();
                for part in parts {
                    match part {
                        InterpPart::Text(t) => result.push_str(t),
                        InterpPart::Expr(e) => {
                            result
                                .push_str(&format!("${{{}}}", Self::compile_expr_to_field_path(e)));
                        }
                    }
                }
                serde_json::json!(result)
            }
        }
    }

    fn compile_expr_to_field_path(expr: &Expr) -> String {
        match expr {
            Expr::Var(name) => name.clone(),
            Expr::Member { object, property } => {
                format!("{}.{}", Self::compile_expr_to_field_path(object), property)
            }
            _ => "value".to_string(),
        }
    }

    #[allow(dead_code)]
    fn compile_condition_to_yaml(expr: &Expr) -> serde_json::Value {
        let _ = expr;
        serde_json::json!({
            "field": "value",
            "operator": "EQ",
            "value": true
        })
    }
}

fn slugify(s: &str) -> String {
    s.to_lowercase()
        .replace(' ', "-")
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-')
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compile_simple_workflow() {
        let program = FlowProgram {
            globals: vec![],
            functions: vec![],
            workflows: vec![WorkflowDef {
                name: "Test Workflow".to_string(),
                event: "TEST_EVENT".to_string(),
                body: vec![Stmt::Log(Expr::string("Hello World"))],
            }],
        };

        let rules = FlowCompiler::compile(&program).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].on, "TEST_EVENT");
    }
}
