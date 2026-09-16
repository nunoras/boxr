use crate::clock;
use crate::output::Toon;
use anyhow::{anyhow, Context, Result};
use duckdb::Connection;
use std::path::Path;
use std::time::Duration;

const DIMENSIONS: &[&str] = &["model", "harness", "effort", "profile", "kind"];

pub fn render(home: &Path, by: &str, since: &str) -> Result<String> {
    let dimensions = dimensions(by)?;
    let window = window(since)?;
    let cutoff = clock::iso8601(clock::now_millis().saturating_sub(window.as_millis()));
    let path = home.join(crate::ledger::SUMMARY_FILE);
    let dimension_count = dimensions.len();
    let mut columns = dimensions.clone();
    columns.extend(["sessions", "tokens", "durationMs"]);
    if !path.is_file() {
        let mut toon = Toon::new();
        toon.table_with_numbers("stats", &columns, &[], dimension_count);
        toon.list(
            "help",
            &["Run a boxr launch to add a session to the ledger".to_string()],
        );
        return Ok(toon.render());
    }
    let query = query(&dimensions);
    let connection = Connection::open_in_memory().context("opening an in-memory stats database")?;
    let mut statement = connection
        .prepare(&query)
        .context("preparing the stats query")?;
    let mut rows = statement
        .query([path.to_string_lossy().as_ref(), cutoff.as_str()])
        .context("reading the session summary ledger")?;
    let mut values = Vec::new();
    while let Some(row) = rows.next().context("reading stats rows")? {
        let mut value = Vec::new();
        for index in 0..dimensions.len() {
            value.push(row.get::<_, String>(index)?);
        }
        value.push(row.get::<_, i64>(dimensions.len())?.to_string());
        value.push(row.get::<_, i64>(dimensions.len() + 1)?.to_string());
        value.push(row.get::<_, i64>(dimensions.len() + 2)?.to_string());
        values.push(value);
    }
    let mut toon = Toon::new();
    toon.table_with_numbers("stats", &columns, &values, dimension_count);
    toon.list(
        "help",
        &["Run `boxr stats --by model,kind --since 30d` to compare a longer window".to_string()],
    );
    Ok(toon.render())
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

fn query(dimensions: &[&str]) -> String {
    let fields: Vec<String> = dimensions
        .iter()
        .map(|dimension| field(dimension))
        .collect();
    let groups = (1..=dimensions.len())
        .map(|index| index.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "SELECT {}, COUNT(*)::BIGINT, SUM(promptTokens + completionTokens + cachedTokens)::BIGINT, SUM(durationMs)::BIGINT FROM read_json_auto(?) WHERE CAST(start AS TIMESTAMP) >= CAST(? AS TIMESTAMP) GROUP BY {} ORDER BY {}",
        fields.join(", "), groups, groups
    )
}

fn field(dimension: &str) -> String {
    match dimension {
        "effort" => "COALESCE(effort, 'harness-default') AS effort".to_string(),
        "profile" => "COALESCE(profile, 'default') AS profile".to_string(),
        "kind" => "COALESCE(kind, 'unclassified') AS kind".to_string(),
        _ => dimension.to_string(),
    }
}
