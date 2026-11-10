use anyhow::{bail, Result};
use regex::Regex;
use std::collections::HashSet;

const MAX_BATCH_URLS: usize = 10_000;

/// Expand FDM-style batch URL patterns:
/// - Numeric ranges with optional zero-padding and step: `[1-10]`, `[001-020:2]`
/// - Single-character alphabetic ranges: `[a-f]`, `[A-Z:2]`
/// - Alternation lists: `{linux,macos,windows}`
/// Supports multiple patterns in one template via ordered Cartesian expansion.
pub fn expand_batch_pattern(template: &str) -> Result<Vec<String>> {
    let input = template.trim();
    if input.is_empty() {
        return Ok(Vec::new());
    }

    let token_re = Regex::new(r"(\[[^\]]+\]|\{[^}]+\})").unwrap();
    let mut results = vec![String::new()];
    let mut last_end = 0;

    for m in token_re.find_iter(input) {
        let literal = &input[last_end..m.start()];
        for r in &mut results {
            r.push_str(literal);
        }

        let token = m.as_str();
        let choices = parse_token(token)?;
        if results.len().saturating_mul(choices.len()) > MAX_BATCH_URLS {
            bail!(
                "Batch expansion exceeds maximum limit of {} URLs",
                MAX_BATCH_URLS
            );
        }

        let mut next = Vec::with_capacity(results.len() * choices.len());
        for prefix in &results {
            for choice in &choices {
                next.push(format!("{prefix}{choice}"));
            }
        }
        results = next;
        last_end = m.end();
    }

    let tail = &input[last_end..];
    for r in &mut results {
        r.push_str(tail);
    }

    Ok(dedupe_preserve_order(results))
}

fn parse_token(token: &str) -> Result<Vec<String>> {
    if token.starts_with('{') && token.ends_with('}') {
        let inner = &token[1..token.len() - 1];
        let parts: Vec<String> = inner
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if parts.is_empty() {
            return Ok(vec![token.to_string()]);
        }
        return Ok(parts);
    }

    if token.starts_with('[') && token.ends_with(']') {
        let inner = &token[1..token.len() - 1];
        let (range_part, step) = if let Some((l, r)) = inner.split_once(':') {
            let s: usize = r.trim().parse().unwrap_or(1).max(1);
            (l.trim(), s)
        } else {
            (inner.trim(), 1)
        };

        if let Some((start_s, end_s)) = range_part.split_once('-') {
            let start_s = start_s.trim();
            let end_s = end_s.trim();

            if let (Ok(start_n), Ok(end_n)) = (start_s.parse::<usize>(), end_s.parse::<usize>()) {
                let width = if start_s.starts_with('0') && start_s.len() > 1 {
                    start_s.len()
                } else if end_s.starts_with('0') && end_s.len() > 1 {
                    end_s.len()
                } else {
                    0
                };
                let mut out = Vec::new();
                if start_n <= end_n {
                    let mut cur = start_n;
                    while cur <= end_n {
                        if width > 0 {
                            out.push(format!("{cur:0width$}"));
                        } else {
                            out.push(cur.to_string());
                        }
                        if out.len() > MAX_BATCH_URLS {
                            bail!("Numeric batch range too large");
                        }
                        cur = cur.saturating_add(step);
                        if step == 0 {
                            break;
                        }
                    }
                } else {
                    let mut cur = start_n;
                    loop {
                        if width > 0 {
                            out.push(format!("{cur:0width$}"));
                        } else {
                            out.push(cur.to_string());
                        }
                        if out.len() > MAX_BATCH_URLS || cur < end_n.saturating_add(step) {
                            if cur >= end_n + step {
                                cur -= step;
                                continue;
                            }
                            if cur > end_n && cur.saturating_sub(step) >= end_n {
                                cur -= step;
                                continue;
                            }
                            break;
                        }
                        cur -= step;
                    }
                }
                return Ok(out);
            }

            // Character range e.g. [a-f]
            let sc: Vec<char> = start_s.chars().collect();
            let ec: Vec<char> = end_s.chars().collect();
            if sc.len() == 1 && ec.len() == 1 {
                let a = sc[0] as u32;
                let b = ec[0] as u32;
                let mut out = Vec::new();
                if a <= b {
                    let mut cur = a;
                    while cur <= b {
                        if let Some(c) = char::from_u32(cur) {
                            out.push(c.to_string());
                        }
                        cur += step as u32;
                    }
                } else {
                    let mut cur = a;
                    while cur >= b {
                        if let Some(c) = char::from_u32(cur) {
                            out.push(c.to_string());
                        }
                        if cur < b + step as u32 {
                            break;
                        }
                        cur -= step as u32;
                    }
                }
                return Ok(out);
            }
        }
    }

    Ok(vec![token.to_string()])
}

/// Parse newline-separated URLs or HTML anchor/plain links from pasted clipboard / files.
pub fn parse_batch_text_input(input: &str) -> Result<Vec<String>> {
    let mut collected = Vec::new();
    for line in input.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let expanded = expand_batch_pattern(trimmed)?;
        for candidate in expanded {
            if url::Url::parse(&candidate).is_ok() {
                collected.push(candidate);
            }
        }
    }
    Ok(dedupe_preserve_order(collected))
}

fn dedupe_preserve_order(items: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        if seen.insert(item.clone()) {
            out.push(item);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_numeric_alpha_and_braces() {
        let urls = expand_batch_pattern("https://cdn.test/{img,thumb}_[01-05:2]_[a-b].png").unwrap();
        assert_eq!(
            urls,
            vec![
                "https://cdn.test/img_01_a.png",
                "https://cdn.test/img_01_b.png",
                "https://cdn.test/img_03_a.png",
                "https://cdn.test/img_03_b.png",
                "https://cdn.test/img_05_a.png",
                "https://cdn.test/img_05_b.png",
                "https://cdn.test/thumb_01_a.png",
                "https://cdn.test/thumb_01_b.png",
                "https://cdn.test/thumb_03_a.png",
                "https://cdn.test/thumb_03_b.png",
                "https://cdn.test/thumb_05_a.png",
                "https://cdn.test/thumb_05_b.png",
            ]
        );
    }

    #[test]
    fn parses_multiline_batch_urls() {
        let urls = parse_batch_text_input(
            "# comment\nhttps://example.org/a.zip\nhttps://example.org/b_[1-2].tar.gz\nnot a url\n",
        )
        .unwrap();
        assert_eq!(
            urls,
            vec![
                "https://example.org/a.zip",
                "https://example.org/b_1.tar.gz",
                "https://example.org/b_2.tar.gz",
            ]
        );
    }
}
