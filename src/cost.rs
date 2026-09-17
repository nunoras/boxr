use crate::config::{Config, Currency, Price};
use anyhow::Result;
use std::collections::BTreeMap;
use std::path::Path;

const TOKENS_PER_PRICE_UNIT: f64 = 1_000_000.0;
const DISPLAY_PLACES: usize = 6;

#[derive(Debug, Clone, Copy)]
pub struct Tokens {
    pub prompt: u64,
    pub completion: u64,
    pub cached: u64,
    pub reasoning: u64,
}

#[derive(Debug, Clone)]
pub struct Pricing {
    pub api_equivalent_cost: Option<f64>,
    pub currency: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Default)]
pub struct CostTable {
    currency: Currency,
    prices: BTreeMap<String, Price>,
}

impl CostTable {
    pub fn new(config: &Config) -> CostTable {
        CostTable {
            currency: config.currency,
            prices: config.prices.clone(),
        }
    }

    pub fn currency(&self) -> &'static str {
        self.currency.code()
    }

    pub fn cost_of(&self, model: &str, tokens: &Tokens) -> Option<f64> {
        let price = self.prices.get(model)?;
        let uncached_input = tokens.prompt.saturating_sub(tokens.cached);
        let non_reasoning_output = tokens.completion.saturating_sub(tokens.reasoning);
        let amount = (uncached_input as f64 / TOKENS_PER_PRICE_UNIT) * price.input
            + (tokens.cached as f64 / TOKENS_PER_PRICE_UNIT) * price.cached
            + (non_reasoning_output as f64 / TOKENS_PER_PRICE_UNIT) * price.output
            + (tokens.reasoning as f64 / TOKENS_PER_PRICE_UNIT) * price.reasoning;
        amount.is_finite().then_some(amount)
    }
}

pub fn table(home: &Path) -> Result<CostTable> {
    Ok(CostTable::new(&Config::load(home)?))
}

pub fn calculate(home: &Path, model: &str, tokens: &Tokens) -> Pricing {
    match table(home) {
        Ok(table) => Pricing {
            api_equivalent_cost: table.cost_of(model, tokens),
            currency: Some(table.currency().to_string()),
            error: None,
        },
        Err(error) => Pricing {
            api_equivalent_cost: None,
            currency: None,
            error: Some(format!("{error:#}")),
        },
    }
}

pub fn render(cost: Option<f64>) -> String {
    cost.map(amount).unwrap_or_else(|| "unknown".to_string())
}

fn amount(value: f64) -> String {
    let fixed = format!("{value:.DISPLAY_PLACES$}");
    if value != 0.0 && fixed == "0.000000" {
        return value.to_string();
    }
    let Some((whole, fraction)) = fixed.split_once('.') else {
        return fixed;
    };
    let trimmed = fraction.trim_end_matches('0');
    let fraction = format!("{trimmed:0<2}");
    format!("{whole}.{fraction}")
}
