use crate::config::{Config, Currency, Price};
use anyhow::Result;
use std::collections::BTreeMap;
use std::fmt;
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
    pub unpriced: bool,
}

#[derive(Debug)]
pub enum CostError {
    MissingPrice(String),
    Arithmetic(anyhow::Error),
}

impl fmt::Display for CostError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CostError::MissingPrice(model) => {
                write!(formatter, "the price table has no entry for model {model}")
            }
            CostError::Arithmetic(error) => write!(formatter, "{error:#}"),
        }
    }
}

impl std::error::Error for CostError {}

impl From<anyhow::Error> for CostError {
    fn from(error: anyhow::Error) -> CostError {
        CostError::Arithmetic(error)
    }
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

    pub fn cost_of(&self, model: &str, tokens: &Tokens) -> Result<f64, CostError> {
        let Some(price) = self.prices.get(model) else {
            return Err(CostError::MissingPrice(model.to_string()));
        };
        let uncached_input = tokens.prompt.saturating_sub(tokens.cached);
        let non_reasoning_output = tokens.completion.saturating_sub(tokens.reasoning);
        let amount = component(uncached_input, price.input, "input")?
            + component(tokens.cached, price.cached, "cached")?
            + component(non_reasoning_output, price.output, "output")?
            + component(tokens.reasoning, price.reasoning, "reasoning")?;
        if !amount.is_finite() {
            return Err(CostError::Arithmetic(anyhow::anyhow!(
                "calculating API-equivalent cost for {model} produced a non-finite amount"
            )));
        }
        Ok(amount)
    }
}

fn component(tokens: u64, price: f64, kind: &str) -> Result<f64> {
    if tokens == 0 || price == 0.0 {
        return Ok(0.0);
    }
    let amount = (tokens as f64 / TOKENS_PER_PRICE_UNIT) * price;
    if amount == 0.0 {
        anyhow::bail!("calculating API-equivalent {kind} cost produced an underflowed amount");
    }
    Ok(amount)
}

pub fn table(home: &Path) -> Result<CostTable> {
    Ok(CostTable::new(&Config::load(home)?))
}

pub fn calculate(home: &Path, model: &str, tokens: &Tokens) -> Pricing {
    match table(home) {
        Ok(table) => match table.cost_of(model, tokens) {
            Ok(api_equivalent_cost) => Pricing {
                api_equivalent_cost: Some(api_equivalent_cost),
                currency: Some(table.currency().to_string()),
                error: None,
                unpriced: false,
            },
            Err(error) => Pricing {
                api_equivalent_cost: None,
                currency: Some(table.currency().to_string()),
                unpriced: matches!(error, CostError::MissingPrice(_)),
                error: Some(error.to_string()),
            },
        },
        Err(error) => Pricing {
            api_equivalent_cost: None,
            currency: None,
            error: Some(format!("{error:#}")),
            unpriced: false,
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
