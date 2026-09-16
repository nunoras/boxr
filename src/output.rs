use std::fmt::{Display, Write as _};

pub const MESSAGE_LIMIT: usize = 200;

pub struct Toon {
    body: String,
}

impl Toon {
    pub fn new() -> Toon {
        Toon {
            body: String::new(),
        }
    }

    pub fn section(&mut self, name: &str) -> &mut Toon {
        let _ = writeln!(self.body, "{name}:");
        self
    }

    pub fn field(&mut self, key: &str, value: &str) -> &mut Toon {
        let _ = writeln!(self.body, "  {key}: {}", scalar(value));
        self
    }

    pub fn number(&mut self, key: &str, value: impl Display) -> &mut Toon {
        let _ = writeln!(self.body, "  {key}: {value}");
        self
    }

    pub fn list(&mut self, name: &str, items: &[String]) -> &mut Toon {
        let _ = writeln!(self.body, "{name}[{}]:", items.len());
        for item in items {
            let _ = writeln!(self.body, "  {item}");
        }
        self
    }

    pub fn render(&self) -> String {
        self.body.clone()
    }

    pub fn table(&mut self, name: &str, columns: &[&str], rows: &[Vec<String>]) -> &mut Toon {
        self.table_with_numbers(name, columns, rows, columns.len())
    }

    pub fn table_with_numbers(
        &mut self,
        name: &str,
        columns: &[&str],
        rows: &[Vec<String>],
        first_number: usize,
    ) -> &mut Toon {
        let _ = writeln!(
            self.body,
            "{name}[{}]{{{}}}:",
            rows.len(),
            columns.join(",")
        );
        for row in rows {
            let cells: Vec<String> = row
                .iter()
                .enumerate()
                .map(|(index, cell)| {
                    if index < first_number {
                        scalar(cell)
                    } else {
                        cell.to_string()
                    }
                })
                .collect();
            let _ = writeln!(self.body, "  {}", cells.join(","));
        }
        self
    }
}

pub fn scalar(value: &str) -> String {
    if needs_quotes(value) {
        format!("\"{}\"", escape(value))
    } else {
        value.to_string()
    }
}

fn needs_quotes(value: &str) -> bool {
    value.is_empty()
        || value.starts_with('-')
        || matches!(value, "true" | "false" | "null")
        || value.parse::<f64>().is_ok()
        || value.chars().any(|c| {
            c.is_whitespace()
                || c.is_control()
                || matches!(c, ',' | ':' | '"' | '\\' | '[' | ']' | '{' | '}')
        })
}

fn escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            _ => escaped.push(c),
        }
    }
    escaped
}

pub fn one_line(text: &str, limit: usize) -> String {
    let flattened = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flattened.chars().count() <= limit {
        return flattened;
    }
    let kept: String = flattened.chars().take(limit).collect();
    format!("{kept}...")
}
