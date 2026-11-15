//! Declarative display rules never change stored cell values.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[non_exhaustive]
pub struct Match {
    pub r#type: Vec<String>,
    pub column: Option<String>,
    pub value: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Display {
    UnixTimestamp {
        unit: String,
        format: String,
    },
    JsonPretty,
    EnumMap {
        values: BTreeMap<String, String>,
    },
    Number {
        decimals: u8,
        #[serde(default)]
        grouping: bool,
    },
    Template {
        template: String,
    },
    Color {
        values: BTreeMap<String, String>,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct Formatter {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    pub r#match: Match,
    pub display: Display,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Presentation {
    text: String,
    tone: Option<String>,
}
impl Presentation {
    pub fn text(&self) -> &str {
        &self.text
    }
    pub fn tone(&self) -> Option<&str> {
        self.tone.as_deref()
    }
}
impl Formatter {
    pub fn validate(&self) -> Result<(), String> {
        for pattern in [&self.r#match.column, &self.r#match.value]
            .into_iter()
            .flatten()
        {
            regex::Regex::new(pattern).map_err(|e| e.to_string())?;
        }
        match &self.display {
            Display::UnixTimestamp { unit, format } => {
                if !["s", "ms"].contains(&unit.as_str())
                    || chrono::format::StrftimeItems::new(format)
                        .any(|item| matches!(item, chrono::format::Item::Error))
                {
                    return Err("Invalid timestamp unit or format.".into());
                }
            }
            Display::Number { decimals, .. } if *decimals > 20 => {
                return Err("Decimal precision must be between 0 and 20.".into());
            }
            Display::Template { template }
                if template.replace("{value}", "").contains(['{', '}']) =>
            {
                return Err("Only {value} is supported in display templates.".into());
            }
            Display::Color { values }
                if values.values().any(|tone| {
                    ![
                        "label",
                        "secondary",
                        "tertiary",
                        "success",
                        "warning",
                        "danger",
                        "info",
                    ]
                    .contains(&tone.as_str())
                }) =>
            {
                return Err("Unknown display color token.".into());
            }
            _ => {}
        }
        Ok(())
    }
    pub fn matches_column(&self, name: &str, type_name: &str) -> bool {
        (self.r#match.r#type.is_empty()
            || self
                .r#match
                .r#type
                .iter()
                .any(|ty| type_name.eq_ignore_ascii_case(ty)))
            && self
                .r#match
                .column
                .as_ref()
                .is_none_or(|pattern| regex::Regex::new(pattern).is_ok_and(|re| re.is_match(name)))
    }
    pub fn apply(&self, name: &str, type_name: &str, value: &str) -> Option<Presentation> {
        if value.len() > 1_048_576 {
            return None;
        }
        if !self.matches_column(name, type_name)
            || self.r#match.value.as_ref().is_some_and(|pattern| {
                !regex::Regex::new(pattern).is_ok_and(|re| re.is_match(value))
            })
        {
            return None;
        }
        let text = match &self.display {
            Display::UnixTimestamp { unit, format } => {
                let stamp = value.parse::<i64>().ok()?;
                let date = if unit == "ms" {
                    chrono::DateTime::from_timestamp_millis(stamp)
                } else {
                    chrono::DateTime::from_timestamp(stamp, 0)
                }?;
                date.format(format).to_string()
            }
            Display::JsonPretty => serde_json::to_string_pretty(
                &serde_json::from_str::<serde_json::Value>(value).ok()?,
            )
            .ok()?,
            Display::EnumMap { values } => values.get(value)?.clone(),
            Display::Number { decimals, grouping } => {
                let n = value.parse::<f64>().ok()?;
                if !n.is_finite() {
                    return None;
                }
                let text = format!("{n:.precision$}", precision = *decimals as usize);
                if *grouping { group_number(&text) } else { text }
            }
            Display::Template { template } => template.replace("{value}", value),
            Display::Color { .. } => value.into(),
        };
        let tone = match &self.display {
            Display::Color { values } => values.get(value).cloned(),
            _ => None,
        };
        Some(Presentation { text, tone })
    }
}
fn group_number(text: &str) -> String {
    let (whole, fraction) = text
        .split_once('.')
        .map_or((text, None), |(whole, fraction)| (whole, Some(fraction)));
    let (sign, digits) = whole
        .strip_prefix('-')
        .map_or(("", whole), |digits| ("-", digits));
    let mut out = sign.to_owned();
    for (ix, ch) in digits.chars().enumerate() {
        if ix > 0 && (digits.len() - ix).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    if let Some(fraction) = fraction {
        out.push('.');
        out.push_str(fraction);
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn six_rules_preserve_values_and_invalid_rules_fail() {
        for (display, value, expected) in [
            (
                r#"{"kind":"unix_timestamp","unit":"s","format":"%Y-%m-%d"}"#,
                "0",
                "1970-01-01",
            ),
            (
                r#"{"kind":"json_pretty"}"#,
                r#"{"a":1}"#,
                "{\n  \"a\": 1\n}",
            ),
            (r#"{"kind":"enum_map","values":{"1":"yes"}}"#, "1", "yes"),
            (
                r#"{"kind":"number","decimals":2,"grouping":true}"#,
                "-1234.5",
                "-1,234.50",
            ),
            (r#"{"kind":"template","template":"${value}"}"#, "12", "$12"),
            (r#"{"kind":"color","values":{"ok":"success"}}"#, "ok", "ok"),
        ] {
            let formatter: Formatter =
                serde_json::from_str(&format!(r#"{{"id":"f","match":{{}},"display":{display}}}"#))
                    .unwrap();
            formatter.validate().unwrap();
            assert_eq!(
                formatter.apply("n", "TEXT", value).unwrap().text(),
                expected
            );
        }
        for display in [
            r#"{"kind":"number","decimals":21}"#,
            r#"{"kind":"template","template":"{sql}"}"#,
            r##"{"kind":"color","values":{"1":"#fff"}}"##,
        ] {
            let formatter: Formatter =
                serde_json::from_str(&format!(r#"{{"id":"f","match":{{}},"display":{display}}}"#))
                    .unwrap();
            assert!(formatter.validate().is_err());
        }
    }
}
