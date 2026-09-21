use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const SCHEMA_VERSION: &str = "ATIF-v1.8";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub name: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Header {
    pub schema_version: String,
    pub session_id: String,
    pub agent: Agent,
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolCall {
    pub tool_call_id: String,
    pub function_name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct ObservationResult {
    pub source_call_id: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Observation {
    pub results: Vec<ObservationResult>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Metrics {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<Map<String, Value>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Step {
    pub step_id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observation: Option<Observation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metrics: Option<Metrics>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<Map<String, Value>>,
}

impl Step {
    pub fn new(source: &str, message: String) -> Step {
        Step {
            step_id: 0,
            timestamp: None,
            source: source.to_string(),
            model_name: None,
            reasoning_effort: None,
            message,
            reasoning_content: None,
            tool_calls: None,
            observation: None,
            metrics: None,
            extra: None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FinalMetrics {
    pub total_prompt_tokens: u64,
    pub total_completion_tokens: u64,
    pub total_cached_tokens: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_cost_usd: Option<f64>,
    pub total_steps: u64,
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Closing {
    pub final_metrics: FinalMetrics,
}

#[derive(Debug, Clone, Serialize)]
pub struct Trajectory {
    pub schema_version: String,
    pub session_id: String,
    pub agent: Agent,
    pub steps: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_metrics: Option<FinalMetrics>,
    pub extra: Map<String, Value>,
}
