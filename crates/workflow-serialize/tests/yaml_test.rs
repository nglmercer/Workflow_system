use workflow_domain::TriggerRule;

#[test]
fn test_parse_yaml() {
    let yaml = r#"
- id: test-rule
  on: TEST_EVENT
  do:
    type: log_message
    params:
      message: "hello"
"#;
    let result: Result<Vec<TriggerRule>, _> = serde_yaml::from_str(yaml);
    match result {
        Ok(rules) => {
            println!("Parsed {} rules", rules.len());
            println!("Rule: {:?}", rules[0]);
        }
        Err(e) => {
            panic!("Failed to parse YAML: {:?}", e);
        }
    }
}
