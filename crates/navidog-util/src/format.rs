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
    fn json_pretty_print() {
        assert_eq!(
            pretty_json("{\"a\":1}").as_deref(),
            Some("{\n  \"a\": 1\n}")
        );
        assert!(pretty_json("not json").is_none());
    }
}
