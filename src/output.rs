use std::fmt::Write as _;

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
}

pub fn scalar(value: &str) -> String {
    let needs_quotes = value.is_empty()
        || value
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, ',' | ':' | '"' | '[' | ']' | '{' | '}'));
    if needs_quotes {
        format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        value.to_string()
    }
}

pub fn one_line(text: &str, limit: usize) -> String {
    let flattened = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flattened.chars().count() <= limit {
        return flattened;
    }
    let kept: String = flattened.chars().take(limit).collect();
    format!("{kept}...")
}
