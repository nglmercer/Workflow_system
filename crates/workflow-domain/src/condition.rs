use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RuleCondition {
    Single(Condition),
    Group(ConditionGroup),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Condition {
    pub field: String,
    pub operator: ComparisonOperator,
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConditionGroup {
    pub operator: LogicOperator,
    pub conditions: Vec<RuleCondition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum LogicOperator {
    And,
    Or,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum ComparisonOperator {
    #[serde(alias = "==")]
    Eq,
    #[serde(alias = "!=")]
    Neq,
    #[serde(alias = ">")]
    Gt,
    #[serde(alias = ">=")]
    Gte,
    #[serde(alias = "<")]
    Lt,
    #[serde(alias = "<=")]
    Lte,
    In,
    #[serde(rename = "NOT_IN", alias = "not_in")]
    NotIn,
    Contains,
    #[serde(rename = "NOT_CONTAINS", alias = "not_contains")]
    NotContains,
    #[serde(rename = "STARTS_WITH", alias = "starts_with")]
    StartsWith,
    #[serde(rename = "ENDS_WITH", alias = "ends_with")]
    EndsWith,
    #[serde(rename = "IS_EMPTY", alias = "is_empty")]
    IsEmpty,
    #[serde(rename = "IS_NULL", alias = "is_null")]
    IsNull,
    #[serde(rename = "IS_NONE", alias = "is_none")]
    IsNone,
    #[serde(rename = "HAS_KEY", alias = "has_key")]
    HasKey,
    Matches,
    Since,
    After,
    Before,
    Until,
    Range,
}
