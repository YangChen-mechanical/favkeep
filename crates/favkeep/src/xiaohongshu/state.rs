use anyhow::{Context, Result, ensure};
use serde_json::Value;

pub fn initial_state(html: &str) -> Result<Value> {
    let marker = regex::Regex::new(r"window\.__INITIAL_STATE__\s*=\s*")?;
    let bytes = &html.as_bytes()[marker.find(html).context("initial state missing")?.end()..];
    ensure!(bytes.first() == Some(&b'{'), "initial state is not an object");
    let (mut quoted, mut escape, mut depth, mut i) = (false, false, 0usize, 0usize);
    let mut json = Vec::new();
    while i < bytes.len() {
        let b = bytes[i];
        // shortcut: normalize only observed state literals; add fixture-backed cases if serialization changes.
        if !quoted && bytes[i..].starts_with(b"new Set([])") {
            json.extend(b"[]");
            i += 11;
            continue;
        }
        if !quoted
            && bytes[i..].starts_with(b"undefined")
            && bytes.get(i + 9).is_some_and(|c| b",}] \r\n\t".contains(c))
        {
            json.extend(b"null");
            i += 9;
            continue;
        }
        json.push(b);
        if quoted {
            if escape {
                escape = false;
            } else if b == b'\\' {
                escape = true;
            } else if b == b'"' {
                quoted = false;
            }
        } else {
            match b {
                b'"' => quoted = true,
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(serde_json::from_slice(&json)?);
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    anyhow::bail!("initial state incomplete")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_observed_state_literals_without_running_scripts_or_altering_strings() {
        let html = r#"<script>window.__INITIAL_STATE__ = {"note":{"x":undefined},"unused":new Set([]),"title":"undefined } and \" braces"};alert('ignored')</script>"#;
        let value = initial_state(html).unwrap();
        assert!(value["note"]["x"].is_null());
        assert_eq!(value["unused"], serde_json::json!([]));
        assert_eq!(value["title"], "undefined } and \" braces");
        assert!(initial_state("window.__INITIAL_STATE__={broken").is_err());
    }
}
