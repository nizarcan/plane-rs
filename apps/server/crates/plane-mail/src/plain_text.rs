// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane.utils.email.generate_plain_text_from_html`
//! (`plane/utils/email.py:16-63`).
//!
//! Django builds every HTML e-mail as `EmailMultiAlternatives(body=
//! generate_plain_text_from_html(html_content))` with the HTML attached as a
//! `text/html` alternative. The plain-text part is therefore part of the wire
//! contract, not a convenience.
//!
//! The two halves are ported literally:
//!
//! * [`remove_style_blocks`] is the forward scan at `:16-33`, including its
//!   `break` on a malformed `<style` or a missing `</style>`.
//! * [`strip_tags`] reproduces `django.utils.html.strip_tags`
//!   (`django/utils/html.py:205-234`): the repeated `_strip_once` loop and
//!   `HTMLParser(convert_charrefs=False)`'s tokenising. Tags are dropped,
//!   comments and declarations dropped, `<script>`/`<style>` contents are data,
//!   and character references are decoded in data. A stray `<` that does not
//!   start a tag is data, as in `HTMLParser`.
//!
//! `html.unescape` (`plane/utils/email.py:55`) runs **after** `strip_tags`, so
//! text that looks like `&amp;lt;` is decoded twice. That is reproduced.
//!
//! Known limit: the named-reference table is the common e-mail subset, not
//! HTML5's full 2,231 entries. Plane's current templates contain no named
//! references; a future template that does needs its names added here.

/// Python `re.compile(r"<style", re.IGNORECASE)` /
/// `re.compile(r"</style>", re.IGNORECASE)`.
///
/// [`remove_style_blocks`] is the `:16-33` loop, byte for byte.
#[must_use]
pub fn remove_style_blocks(html: &str) -> String {
    let lower = html.to_lowercase();
    let mut parts: Vec<&str> = Vec::new();
    let mut pos = 0usize;
    while let Some(start) = lower[pos..].find("<style").map(|index| pos + index) {
        let Some(tag_end_rel) = html[start + "<style".len()..].find('>') else {
            break;
        };
        let tag_end = start + "<style".len() + tag_end_rel;
        let Some(close_rel) = lower[tag_end + 1..].find("</style>") else {
            break;
        };
        let close = tag_end + 1 + close_rel;
        parts.push(&html[pos..start]);
        pos = close + "</style>".len();
    }
    parts.push(&html[pos..]);
    parts.concat()
}

/// `django.utils.html.strip_tags` (`django/utils/html.py:215-234`).
#[must_use]
pub fn strip_tags(value: &str) -> String {
    let mut value = value.to_string();
    // Django's depth cap is 10,000; a loop that does not reduce the tag count
    // stops after the first pass, exactly like `:229-231`.
    let mut depth = 0usize;
    while value.contains('<') && value.contains('>') {
        if depth >= 10_000 {
            break;
        }
        let stripped = strip_once(&value);
        if count_char(&value, '<') == count_char(&stripped, '<') {
            break;
        }
        value = stripped;
        depth += 1;
    }
    value
}

/// `plane.utils.email.generate_plain_text_from_html` (`:36-63`).
#[must_use]
pub fn generate_plain_text_from_html(html_content: &str) -> String {
    let without_styles = remove_style_blocks(html_content);
    let text = html_unescape(&strip_tags(&without_styles));
    let collapsed = collapse_blank_lines(&text);
    format!("\n\n{}\n\n", collapsed.trim())
}

fn count_char(text: &str, needle: char) -> usize {
    text.chars().filter(|ch| *ch == needle).count()
}

/// `re.sub(r"\n\s*\n\s*\n+", "\n\n", text)` (`plane/utils/email.py:58`).
///
/// Python's `\s` is Unicode-aware (`str` patterns), so this is too.
fn collapse_blank_lines(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut index = 0usize;
    while index < chars.len() {
        if chars[index] != '\n' {
            out.push(chars[index]);
            index += 1;
            continue;
        }
        // A run `\n \s* \n \s* \n+`: at least three newlines with only
        // whitespace between them. The leftmost match wins, which is what
        // Python's non-overlapping `re.sub` does.
        let mut cursor = index;
        let mut newlines = 0usize;
        let mut last_newline = index;
        while cursor < chars.len() {
            if chars[cursor] == '\n' {
                newlines += 1;
                last_newline = cursor;
                cursor += 1;
            } else if chars[cursor].is_whitespace() {
                cursor += 1;
            } else {
                break;
            }
        }
        if newlines >= 3 {
            out.push_str("\n\n");
            // Skip everything the pattern consumed, up to and including the
            // last newline of the run.
            index = last_newline + 1;
        } else {
            out.push('\n');
            index += 1;
        }
    }
    out
}

/// `django.utils.html._strip_once` (`django/utils/html.py:205-212`) with
/// `HTMLParser(convert_charrefs=False)` (`MLStripper.__init__`, `:182-187`).
fn strip_once(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let chars: Vec<char> = value.chars().collect();
    let mut index = 0usize;
    while index < chars.len() {
        let ch = chars[index];
        if ch != '<' {
            // `HTMLParser(convert_charrefs=False)` keeps character references
            // verbatim (`MLStripper.handle_entityref`), so `&amp;` is data.
            out.push(ch);
            index += 1;
            continue;
        }

        // `<!-- comment -->`
        if starts_with(&chars[index..], "<!--") {
            if let Some(end) = find_str(&chars[index + 4..], "-->") {
                index += 4 + end + 3;
            } else {
                // An unterminated comment is data in HTMLParser.
                out.push('<');
                index += 1;
            }
            continue;
        }
        // `<!DOCTYPE ...>`, `<![CDATA[...]]>` and `<?...>` are dropped.
        if starts_with(&chars[index..], "<!") || starts_with(&chars[index..], "<?") {
            if let Some(end) = find_char(&chars[index..], '>') {
                index += end + 1;
            } else {
                out.push('<');
                index += 1;
            }
            continue;
        }
        // End tag `</name ...>`.
        if starts_with(&chars[index..], "</") {
            if let Some(end) = find_tag_end(&chars[index..]) {
                index += end;
            } else {
                out.push('<');
                index += 1;
            }
            continue;
        }
        // Start tag `<name ...>`; only an ASCII letter starts one.
        if chars.get(index + 1).is_some_and(char::is_ascii_alphabetic) {
            let Some(end) = find_tag_end(&chars[index..]) else {
                out.push('<');
                index += 1;
                continue;
            };
            let name_end = chars[index + 1..]
                .iter()
                .position(|ch| !(ch.is_ascii_alphanumeric() || *ch == '-'))
                .map(|offset| index + 1 + offset)
                .unwrap_or(index + 1 + chars[index + 1..].len());
            let name: String = chars[index + 1..name_end]
                .iter()
                .collect::<String>()
                .to_ascii_lowercase();
            index += end;
            // `HTMLParser.set_cdata_mode` for script/style: content up to the
            // matching end tag is data. Plane's e-mail templates have no
            // script, but strip_tags is also used on user HTML elsewhere.
            if name == "script" || name == "style" {
                let close = format!("</{name}");
                let lower: String = chars[index..]
                    .iter()
                    .collect::<String>()
                    .to_ascii_lowercase();
                if let Some(offset) = lower.find(&close) {
                    let content_end = index + lower[..offset].chars().count();
                    let content: String = chars[index..content_end].iter().collect();
                    out.push_str(&content);
                    let rest = &chars[content_end..];
                    index = content_end + find_tag_end(rest).unwrap_or(0);
                }
            }
            continue;
        }
        // A lone `<` is data.
        out.push('<');
        index += 1;
    }
    out
}

fn starts_with(chars: &[char], prefix: &str) -> bool {
    let mut iter = prefix.chars();
    for ch in chars {
        let Some(expected) = iter.next() else {
            return true;
        };
        if *ch != expected {
            return false;
        }
    }
    iter.next().is_none()
}

fn find_char(chars: &[char], needle: char) -> Option<usize> {
    chars.iter().position(|ch| *ch == needle)
}

fn find_str(chars: &[char], needle: &str) -> Option<usize> {
    let needle: Vec<char> = needle.chars().collect();
    if needle.is_empty() || chars.len() < needle.len() {
        return None;
    }
    (0..=chars.len() - needle.len()).find(|&start| chars[start..start + needle.len()] == needle[..])
}

/// The offset just past the closing `>` of a tag, honouring quoted attribute
/// values (HTMLParser's `parse_starttag`).
fn find_tag_end(chars: &[char]) -> Option<usize> {
    let mut quote: Option<char> = None;
    let mut index = 0usize;
    while index < chars.len() {
        let ch = chars[index];
        match quote {
            Some(q) if ch == q => quote = None,
            Some(_) => {}
            None if ch == '"' || ch == '\'' => quote = Some(ch),
            None if ch == '>' => return Some(index + 1),
            None => {}
        }
        index += 1;
    }
    None
}

/// `html.unescape` (`html/__init__.py:120-160`).
fn html_unescape(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut index = 0usize;
    while index < chars.len() {
        if chars[index] == '&'
            && let Some((decoded, consumed)) = decode_reference(&chars[index..])
        {
            out.push_str(&decoded);
            index += consumed;
            continue;
        }
        out.push(chars[index]);
        index += 1;
    }
    out
}

/// Decode one `&…` reference at the start of `chars`, returning the replacement
/// text and how many input characters it consumed.
///
/// `html.unescape`'s named-reference match is longest-prefix over HTML5's
/// table, and a name without a semicolon is accepted for the legacy names. The
/// table here is the common e-mail subset plus every legacy name.
fn decode_reference(chars: &[char]) -> Option<(String, usize)> {
    // `&#...;` / `&#x...;`
    if chars.get(1) == Some(&'#') {
        let hex = matches!(chars.get(2), Some('x' | 'X'));
        let start = if hex { 3 } else { 2 };
        let mut end = start;
        while end < chars.len() && chars[end].is_ascii_hexdigit() {
            end += 1;
        }
        if end == start {
            return None;
        }
        // `html.unescape` accepts the digits without a semicolon too.
        if chars.get(end) == Some(&';') {
            end += 1;
        }
        let digits: String = chars[start..end].iter().collect();
        let digits = digits.trim_end_matches(';');
        let code = if hex {
            u32::from_str_radix(digits, 16).ok()
        } else {
            digits.parse::<u32>().ok()
        };
        let value = match code {
            Some(0) | Some(0xD800..=0xDFFF) | Some(0x110000..) | None => '\u{FFFD}',
            Some(code) => char::from_u32(code).unwrap_or('\u{FFFD}'),
        };
        return Some((value.to_string(), end));
    }

    let mut end = 1usize;
    while end < chars.len() && chars[end].is_ascii_alphanumeric() {
        end += 1;
    }
    if end == 1 {
        return None;
    }
    let name: String = chars[1..end].iter().collect();
    let semicolon = chars.get(end) == Some(&';');
    if semicolon {
        let with = format!("{name};");
        if let Some(value) = named_reference(&with) {
            return Some((value.to_string(), end + 1));
        }
    }
    // Longest-prefix match for names without a semicolon (`&notit;` ->
    // `¬it;`).
    for take in (1..=name.len()).rev() {
        let prefix = &name[..take];
        if let Some(value) = named_reference(prefix) {
            return Some((value.to_string(), 1 + take));
        }
    }
    None
}

/// The named-reference subset the e-mail templates can produce, decoded as
/// `html.unescape` does (including names without a final semicolon).
fn named_reference(name: &str) -> Option<&'static str> {
    Some(match name {
        "amp" | "amp;" => "&",
        "lt" | "lt;" => "<",
        "gt" | "gt;" => ">",
        "quot" | "quot;" => "\"",
        "apos;" => "'",
        "nbsp" | "nbsp;" => "\u{a0}",
        "copy" | "copy;" => "©",
        "reg" | "reg;" => "®",
        "trade" | "trade;" => "™",
        "hellip" | "hellip;" => "…",
        "mdash" | "mdash;" => "—",
        "ndash" | "ndash;" => "–",
        "lsquo" | "lsquo;" => "‘",
        "rsquo" | "rsquo;" => "’",
        "ldquo" | "ldquo;" => "“",
        "rdquo" | "rdquo;" => "”",
        "laquo" | "laquo;" => "«",
        "raquo" | "raquo;" => "»",
        "bull" | "bull;" => "•",
        "middot" | "middot;" => "·",
        "times" | "times;" => "×",
        "divide" | "divide;" => "÷",
        "deg" | "deg;" => "°",
        "plusmn" | "plusmn;" => "±",
        "para" | "para;" => "¶",
        "sect" | "sect;" => "§",
        "euro" | "euro;" => "€",
        "pound" | "pound;" => "£",
        "yen" | "yen;" => "¥",
        "cent" | "cent;" => "¢",
        "check" | "check;" => "✓",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_style_blocks_case_insensitively() {
        assert_eq!(
            remove_style_blocks("a<STYLE type=\"x\">p{}</STYLE>b"),
            "ab".to_string()
        );
        assert_eq!(
            remove_style_blocks("a<style>p{}</style>b<style>x"),
            "ab<style>x"
        );
    }

    #[test]
    fn strips_tags_and_decodes_references() {
        assert_eq!(strip_tags("<p>a &amp; b</p>"), "a &amp; b");
        assert_eq!(strip_tags("a < b > c"), "a < b > c");
        assert_eq!(
            strip_tags("<script>if (a < b) x();</script>t"),
            "if (a < b) x();t"
        );
        assert_eq!(strip_tags("<!-- c -->x"), "x");
        assert_eq!(strip_tags("<a title=\"a>b\">x</a>"), "x");
    }

    #[test]
    fn plain_text_wraps_and_collapses() {
        assert_eq!(
            generate_plain_text_from_html("<style>p{}</style><p>hi</p>\n\n\n\nbye"),
            "\n\nhi\n\nbye\n\n"
        );
    }
}
