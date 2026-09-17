use crate::clock;
use crate::cost;
use crate::output::{Kind, Toon};
use anyhow::{anyhow, Context, Result};
use duckdb::Connection;
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

const COLUMNS: &str = "{'id':'VARCHAR','model':'VARCHAR','harness':'VARCHAR','effort':'VARCHAR','profile':'VARCHAR','kind':'VARCHAR','status':'VARCHAR','verdict':'VARCHAR','interrupted':'BOOLEAN','limitHit':'BOOLEAN','start':'VARCHAR','promptTokens':'BIGINT','completionTokens':'BIGINT','cachedTokens':'BIGINT','durationMs':'BIGINT','apiEquivalentCost':'DOUBLE','currency':'VARCHAR','costError':'VARCHAR'}";

const SUMMARY_COLUMNS: &[&str] = &[
    "model",
    "harness",
    "effort",
    "profile",
    "kind",
    "status",
    "verdict",
    "interrupted",
    "limitHit",
    "start",
    "promptTokens",
    "completionTokens",
    "cachedTokens",
    "durationMs",
    "apiEquivalentCost",
    "currency",
    "costError",
];

pub fn render(home: &Path, by: &str, since: &str) -> Result<String> {
    let dimensions = dimensions(by)?;
    let window = window(since)?;
    let cutoff = clock::iso8601(clock::now_millis().saturating_sub(window.as_millis()));
    let path = home.join(crate::ledger::SUMMARY_FILE);
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
            "interrupted"
            | "limitHit"
            | "sessions"
            | "tokens"
            | "durationMs"
            | "apiEquivalentCost"
            | "unpricedSessions" => Kind::Number,
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
            value.push(row.get::<_, Option<String>>(index)?.unwrap_or_default());
        }
        value.push(row.get::<_, String>(dimensions.len())?);
        value.push(row.get::<_, i64>(dimensions.len() + 1)?.to_string());
        value.push(row.get::<_, i64>(dimensions.len() + 2)?.to_string());
        value.push(row.get::<_, i64>(dimensions.len() + 3)?.to_string());
        let api_equivalent_cost = row.get::<_, Option<f64>>(dimensions.len() + 4)?;
        if api_equivalent_cost.is_some_and(|amount| !amount.is_finite()) {
            return Err(anyhow!(
                "calculating API-equivalent cost total produced a non-finite amount"
            ));
        }
        value.push(cost::render(api_equivalent_cost));
        value.push(row.get::<_, i64>(dimensions.len() + 5)?.to_string());
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
    let groups = (1..=dimensions.len() + 1)
        .map(|index| index.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let summaries = SUMMARY_COLUMNS
        .iter()
        .map(|column| {
            let column = column_name(column);
            format!("arg_max({column}, sequence) FILTER (WHERE {column} IS NOT NULL) AS {column}")
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "WITH entries AS (SELECT *, row_number() OVER () AS sequence FROM read_json(?, format='newline_delimited', columns={COLUMNS})), summaries AS (SELECT id, {summaries} FROM entries GROUP BY id) SELECT {}, COALESCE(currency, '{UNKNOWN_CURRENCY}') AS currency, COUNT(*)::BIGINT, SUM(promptTokens + completionTokens + cachedTokens)::BIGINT, SUM(durationMs)::BIGINT, SUM(apiEquivalentCost), COUNT(*) FILTER (WHERE apiEquivalentCost IS NULL AND costError IS NULL)::BIGINT FROM summaries WHERE CAST(start AS TIMESTAMP) >= CAST(? AS TIMESTAMP) GROUP BY {} ORDER BY {}",
        fields.join(", "),
        groups,
        groups
    )
}

fn column_name(column: &str) -> &str {
    if column == "limitHit" {
        "\"limitHit\""
    } else {
        column
    }
}

fn field(dimension: &str) -> String {
    match dimension {
        "effort" => "COALESCE(effort, 'harness-default') AS effort".to_string(),
        "profile" => "COALESCE(profile, 'default') AS profile".to_string(),
        "kind" => "COALESCE(kind, 'unclassified') AS kind".to_string(),
        "verdict" => "COALESCE(verdict, 'none') AS verdict".to_string(),
        "interrupted" => "CAST(COALESCE(interrupted, false) AS VARCHAR) AS interrupted".to_string(),
        "limitHit" => "CAST(COALESCE(\"limitHit\", false) AS VARCHAR) AS \"limitHit\"".to_string(),
        _ => dimension.to_string(),
    }
}
