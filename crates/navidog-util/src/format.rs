//! Text conversions that do not belong to a single feature.

/// Bytes of `text` as lowercase hex pairs separated by spaces.
pub fn encode_hex(text: &str) -> String {
    let encoded = hex::encode(text.as_bytes());
    encoded
        .as_bytes()
        .chunks(2)
        .map(|pair| std::str::from_utf8(pair).unwrap_or_default())
        .collect::<Vec<_>>()
        .join(" ")
}

/// `data_length` as Navicat shows it: 1024-based, exact values without a fraction (`16 KB`).
pub fn format_bytes(text: &str) -> String {
    let Ok(n) = text.trim().parse::<u64>() else {
        return text.to_string();
    };
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1024. && unit + 1 < UNITS.len() {
        value /= 1024.;
        unit += 1;
    }
    if unit == 0 {
        return format!("{n} B");
    }
    if value.fract() == 0. {
        format!("{} {}", value as u64, UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// `None` when the hex is malformed or the bytes are not UTF-8.
pub fn decode_hex(text: &str) -> Option<String> {
    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    let bytes = hex::decode(compact).ok()?;
    String::from_utf8(bytes).ok()
}

pub fn pretty_json(text: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(text.trim()).ok()?;
    serde_json::to_string_pretty(&value).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips_and_rejects_bad_bytes() {
        let text = "Pénélope\n";
        let hex = encode_hex(text);
        assert_eq!(decode_hex(&hex).as_deref(), Some(text));
        assert_eq!(decode_hex("70 65"), Some("pe".into()));
        assert!(decode_hex("zz").is_none());
        assert!(decode_hex("f").is_none());
        assert!(decode_hex("ff").is_none());
    }

    #[test]
    fn byte_sizes_use_1024_and_drop_a_zero_fraction() {
        assert_eq!(format_bytes("16384"), "16 KB");
        assert_eq!(format_bytes("1536"), "1.5 KB");
        assert_eq!(format_bytes("12"), "12 B");
        assert_eq!(format_bytes("n/a"), "n/a");
    }

    #[test]
    fn json_pretty_print() {
        assert_eq!(
            pretty_json("{\"a\":1}").as_deref(),
            Some("{\n  \"a\": 1\n}")
        );
        assert!(pretty_json("not json").is_none());
    }
}
