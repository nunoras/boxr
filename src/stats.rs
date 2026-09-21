use crate::clock;
use crate::cost;
use crate::ledger::SUMMARY_FILE;
use crate::output::{Kind, Toon};
use anyhow::{anyhow, Context, Result};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::Duration;

const DIMENSIONS: &[&str] = &[
    "model",
    "harness",
    "effort",
    "profile",
    "kind",
    "status",
    "verdict",
    "interrupted",
    "limitHit",
];

const UNKNOWN_CURRENCY: &str = "unknown";

pub fn render(home: &Path, by: &str, since: &str) -> Result<String> {
    let dimensions = dimensions(by)?;
    let window = window(since)?;
    let cutoff = clock::iso8601(clock::now_millis().saturating_sub(window.as_millis()));
    let path = home.join(SUMMARY_FILE);
    let mut columns = dimensions.clone();
    columns.extend([
        "currency",
        "sessions",
        "tokens",
        "durationMs",
        "apiEquivalentCost",
        "unpricedSessions",
    ]);
    let kinds = columns
        .iter()
        .map(|column| match *column {
            "interrupted" | "limitHit" | "sessions" | "tokens" | "durationMs"
            | "apiEquivalentCost" | "unpricedSessions" => Kind::Number,
            _ => Kind::Text,
        })
        .collect::<Vec<_>>();
    if !path.is_file() {
        let mut toon = Toon::new();
        toon.table("stats", &columns, &[], &kinds);
        toon.list(
            "help",
            &["Run a boxr launch to add a session to the ledger".to_string()],
        );
        return Ok(toon.render());
    }
    let text = fs::read_to_string(&path)
        .with_context(|| format!("reading the session summary ledger at {}", path.display()))?;
    let mut groups: BTreeMap<Vec<String>, Aggregate> = BTreeMap::new();
    for summary in fold_summaries(&text) {
        if !in_window(&summary, &cutoff) {
            continue;
        }
        let mut key = dimensions
            .iter()
            .map(|dimension| dimension_value(&summary, dimension))
            .collect::<Vec<_>>();
        key.push(currency_value(&summary));
        let aggregate = groups.entry(key).or_default();
        aggregate.sessions += 1;
        aggregate.tokens += token_total(&summary);
        aggregate.duration_ms += integer_field(&summary, "durationMs");
        if let Some(amount) = float_field(&summary, "apiEquivalentCost") {
            aggregate.api_equivalent_cost =
                Some(aggregate.api_equivalent_cost.unwrap_or(0.0) + amount);
        }
        if float_field(&summary, "apiEquivalentCost").is_none()
            && (bool_field(&summary, "costUnpriced")
                || string_field(&summary, "costError").is_none())
        {
            aggregate.unpriced_sessions += 1;
        }
    }
    let mut values = Vec::new();
    for (key, aggregate) in groups {
        if aggregate
            .api_equivalent_cost
            .is_some_and(|amount| !amount.is_finite())
        {
            return Err(anyhow!(
                "calculating API-equivalent cost total produced a non-finite amount"
            ));
        }
        let mut value = key;
        value.push(aggregate.sessions.to_string());
        value.push(aggregate.tokens.to_string());
        value.push(aggregate.duration_ms.to_string());
        value.push(cost::render(aggregate.api_equivalent_cost));
        value.push(aggregate.unpriced_sessions.to_string());
        values.push(value);
    }
    let mut toon = Toon::new();
    toon.table("stats", &columns, &values, &kinds);
    toon.list(
        "help",
        &["Run `boxr stats --by model,kind --since 30d` to compare a longer window".to_string()],
    );
    Ok(toon.render())
}

#[derive(Default)]
struct Aggregate {
    sessions: u64,
    tokens: u64,
    duration_ms: u64,
    api_equivalent_cost: Option<f64>,
    unpriced_sessions: u64,
}

fn fold_summaries(text: &str) -> Vec<Map<String, Value>> {
    let mut order = Vec::new();
    let mut by_id: BTreeMap<String, Map<String, Value>> = BTreeMap::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let Ok(Value::Object(record)) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Some(id) = record.get("id").and_then(Value::as_str).map(str::to_owned) else {
            continue;
        };
        let entry = by_id.entry(id.clone()).or_insert_with(|| {
            order.push(id);
            Map::new()
        });
        for (key, value) in record {
            if !value.is_null() {
                entry.insert(key, value);
            }
        }
    }
    order
        .into_iter()
        .filter_map(|id| by_id.remove(&id))
        .collect()
}

fn in_window(summary: &Map<String, Value>, cutoff: &str) -> bool {
    string_field(summary, "start").is_some_and(|start| start.as_str() >= cutoff)
}

fn dimension_value(summary: &Map<String, Value>, dimension: &str) -> String {
    match dimension {
        "effort" => string_field(summary, "effort").unwrap_or_else(|| "harness-default".into()),
        "profile" => string_field(summary, "profile").unwrap_or_else(|| "default".into()),
        "kind" => string_field(summary, "kind").unwrap_or_else(|| "unclassified".into()),
        "verdict" => string_field(summary, "verdict").unwrap_or_else(|| "none".into()),
        "interrupted" => bool_field(summary, "interrupted").to_string(),
        "limitHit" => bool_field(summary, "limitHit").to_string(),
        "model" => string_field(summary, "model").unwrap_or_default(),
        "harness" => string_field(summary, "harness").unwrap_or_default(),
        "status" => string_field(summary, "status").unwrap_or_default(),
        _ => string_field(summary, dimension).unwrap_or_default(),
    }
}

fn currency_value(summary: &Map<String, Value>) -> String {
    string_field(summary, "currency").unwrap_or_else(|| UNKNOWN_CURRENCY.into())
}

fn token_total(summary: &Map<String, Value>) -> u64 {
    integer_field(summary, "promptTokens")
        + integer_field(summary, "completionTokens")
        + integer_field(summary, "cachedTokens")
}

fn string_field(summary: &Map<String, Value>, key: &str) -> Option<String> {
    match summary.get(key)? {
        Value::String(value) => Some(value.clone()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Number(value) => Some(value.to_string()),
        Value::Null => None,
        _ => None,
    }
}

fn bool_field(summary: &Map<String, Value>, key: &str) -> bool {
    match summary.get(key) {
        Some(Value::Bool(value)) => *value,
        Some(Value::String(value)) => value == "true",
        _ => false,
    }
}

fn integer_field(summary: &Map<String, Value>, key: &str) -> u64 {
    match summary.get(key) {
        Some(Value::Number(value)) => value
            .as_u64()
            .or_else(|| value.as_i64().map(|value| value.max(0) as u64))
            .or_else(|| value.as_f64().map(|value| value.max(0.0) as u64))
            .unwrap_or(0),
        Some(Value::String(value)) => value.parse().unwrap_or(0),
        _ => 0,
    }
}

fn float_field(summary: &Map<String, Value>, key: &str) -> Option<f64> {
    match summary.get(key)? {
        Value::Number(value) => value.as_f64(),
        Value::String(value) => value.parse().ok(),
        Value::Null => None,
        _ => None,
    }
}

fn dimensions(by: &str) -> Result<Vec<&'static str>> {
    let values: Vec<&str> = by
        .split(',')
        .filter(|dimension| !dimension.is_empty())
        .collect();
    if values.is_empty()
        || values
            .iter()
            .any(|dimension| !DIMENSIONS.contains(dimension))
    {
        return Err(anyhow!(
            "--by needs one or more of: {}",
            DIMENSIONS.join(", ")
        ));
    }
    if values.len()
        != values
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
    {
        return Err(anyhow!("--by cannot repeat a dimension"));
    }
    Ok(values
        .iter()
        .map(|value| {
            DIMENSIONS
                .iter()
                .find(|dimension| *dimension == value)
                .copied()
                .unwrap()
        })
        .collect())
}

fn window(value: &str) -> Result<Duration> {
    let (number, unit) = value.split_at(value.len().saturating_sub(1));
    let number: u64 = number
        .parse()
        .map_err(|_| anyhow!("--since needs a window such as 7d"))?;
    let seconds = match unit {
        "m" => number.saturating_mul(60),
        "h" => number.saturating_mul(60 * 60),
        "d" => number.saturating_mul(24 * 60 * 60),
        _ => return Err(anyhow!("--since needs a window such as 7d")),
    };
    Ok(Duration::from_secs(seconds))
}
