pub(super) fn tsv_field(value: &str) -> String {
    if value.contains(['\t', '\n', '\r', '"']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

/// Rows of quoted-or-plain TSV. A quoted field may contain tabs, newlines and
/// doubled quotes; CRLF input is accepted.
pub(super) fn end_tsv_row(row: &mut Vec<String>, field: &mut String, rows: &mut Vec<Vec<String>>) {
    row.push(std::mem::take(field));
    rows.push(std::mem::take(row));
}

pub(super) fn parse_tsv(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut started = false;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if quoted {
            match ch {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => quoted = false,
                other => field.push(other),
            }
            continue;
        }
        match ch {
            '"' if field.is_empty() => {
                quoted = true;
                started = true;
            }
            '\t' => {
                row.push(std::mem::take(&mut field));
                started = false;
            }
            '\r' if chars.peek() == Some(&'\n') => {
                // CRLF is a row delimiter; a CR at the end of a quoted field was
                // already pushed while `quoted` was true and must survive.
                chars.next();
                end_tsv_row(&mut row, &mut field, &mut rows);
                started = false;
            }
            '\n' => {
                end_tsv_row(&mut row, &mut field, &mut rows);
                started = false;
            }
            other => {
                field.push(other);
                started = true;
            }
        }
    }
    if started || !row.is_empty() {
        end_tsv_row(&mut row, &mut field, &mut rows);
    }
    rows
}

pub(super) fn contains(hay: &str, needle: &str, case_sensitive: bool) -> bool {
    if needle.is_empty() {
        return false;
    }
    if case_sensitive {
        hay.contains(needle)
    } else if hay.is_ascii() && needle.is_ascii() {
        // The usual case, without the per-cell `Vec<char>`s below (the grid checks every visible cell)
        let n = needle.as_bytes();
        hay.as_bytes()
            .windows(n.len())
            .any(|w| w.eq_ignore_ascii_case(n))
    } else {
        let n: Vec<char> = needle.chars().collect();
        let h: Vec<char> = hay.chars().collect();
        h.windows(n.len()).any(|w| {
            w.iter()
                .zip(&n)
                .all(|(a, b)| a.to_lowercase().eq(b.to_lowercase()))
        })
    }
}

pub(super) fn replace_text(
    hay: &str,
    needle: &str,
    replacement: &str,
    case_sensitive: bool,
) -> Option<String> {
    if needle.is_empty() || !contains(hay, needle, case_sensitive) {
        return None;
    }
    if case_sensitive {
        return Some(hay.replace(needle, replacement));
    }
    let h: Vec<char> = hay.chars().collect();
    let n: Vec<char> = needle.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < h.len() {
        let hit = i + n.len() <= h.len()
            && h[i..i + n.len()]
                .iter()
                .zip(&n)
                .all(|(a, b)| a.to_lowercase().eq(b.to_lowercase()));
        if hit {
            out.push_str(replacement);
            i += n.len();
        } else {
            out.push(h[i]);
            i += 1;
        }
    }
    Some(out)
}
