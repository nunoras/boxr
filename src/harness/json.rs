use serde_json::Value;

pub fn joined_text(parts: &[Value]) -> String {
    parts
        .iter()
        .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn joined_reasoning(parts: &[Value]) -> Option<String> {
    let thoughts = parts
        .iter()
        .filter(|part| part.get("type").and_then(Value::as_str) == Some("thinking"))
        .filter_map(|part| part.get("thinking").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n");
    (!thoughts.is_empty()).then_some(thoughts)
}

pub fn number_at(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or(0)
}

pub fn text_at(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|text| !text.is_empty())
}
