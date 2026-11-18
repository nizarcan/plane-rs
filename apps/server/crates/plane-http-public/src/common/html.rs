// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The `lxml` parse gate `/api/v1/` puts in front of `description_html`.
//!
//! `plane/api/serializers/issue.py:102-109`:
//!
//! ```python
//! try:
//!     if data.get("description_html", None) is not None:
//!         parsed = html.fromstring(data["description_html"])
//!         parsed_str = html.tostring(parsed, encoding="unicode")
//!         data["description_html"] = parsed_str
//! except Exception:
//!     raise serializers.ValidationError("Invalid HTML passed")
//! ```
//!
//! `validate_html_content` sanitises the result with `nh3` afterwards
//! (`plane/utils/content_validator.py:209-239`), so the stored value is
//! `nh3.clean(lxml_normalized)`. [`normalize`] returns the middle step and
//! `plane_http_app::write::html::sanitize` the last one; the corpus
//! (`tests/golden/lxml_html.jsonl`) records both.
//!
//! ## What `html.fromstring` does
//!
//! `lxml.html.fromstring` runs libxml2's HTML 4 parser in recover mode
//! (`document_fromstring`), then reshapes the result
//! (`lxml/html/__init__.py:839-904`):
//!
//! * an input that looks like a whole document (`^\s*<(?:html|!doctype)`) is
//!   returned as the document's `<html>` element;
//! * a document with a `<head>` (only `script`, `style`, `meta`, `link`,
//!   `title` and `base` before any body content create one) is returned whole;
//! * a `<body>` holding exactly one element, with blank text around it, is
//!   returned as that element (with the blank tail it had inside the body);
//! * every other body is returned renamed to `<div>` when it contains a block
//!   level tag and to `<span>` otherwise.
//!
//! `html.tostring(..., encoding="unicode")` is libxml2's HTML serialiser with
//! `format=0` (`lxml/serializer.pxi:_writeNodeToBuffer` →
//! `htmlNodeDumpFormatOutput`), as patched in the lxml wheel: `&`, `<` and `>`
//! are escaped in text and attribute values, `"` is escaped only when the
//! value also contains `'` (then the value keeps `"` quoting), single quotes
//! are used when the value contains `"` but no `'`, void tags have no closing
//! tag, an empty `<li>` is written `<li>`, and `href`/`src`/`action` (plus
//! `name` on `<a>`) are URI-escaped after their leading blanks are dropped.
//!
//! ## Fidelity and limits
//!
//! The parser reproduces libxml2 2.14.6's (the version in the pinned `lxml`)
//! tree construction: the implied `html`/`head`/`body` elements
//! (`HTMLparser.c:htmlCheckImplied`), the `htmlStartClose` auto-close table,
//! the `htmlEndPriority` end-tag rule, the empty-element table, the
//! `DATA_SCRIPT`/`DATA_RAWTEXT`/`DATA_RCDATA` text modes, CR/LF normalisation,
//! NUL → U+FFFD, and the named/numeric reference rules (`html5ent.inc`).
//!
//! Two deliberate limits, both outside what Plane's editor emits and both
//! recorded here rather than hidden:
//!
//! * the named-entity table is HTML 4's 253 names (`html40EntitiesTable`)
//!   rather than libxml2's 2 125-name HTML5 table, so an HTML5-only name such
//!   as `&check;` stays literal text where lxml decodes it;
//! * libxml2's script-escape states (`<!-- … -->` inside `<script>`) are not
//!   modelled: a `</script>` inside a script comment ends the element. `nh3`
//!   removes `<script>` content either way, so the stored value is unaffected.
//!
//! Everything the corpus covers — including implied end tags, entity decoding,
//! attribute quoting, URI escaping and multi-root wrapping — is exact, and
//! `tests/lxml_html.rs` fails on any difference.

use plane_core::error::ApiError;
use plane_drf::error::ViewError;

/// The `ValidationError` detail `plane/api/serializers/issue.py:109` raises.
pub const INVALID_HTML: &str = "Invalid HTML passed";

/// Why the gate refused a `description_html` value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidHtml;

impl std::fmt::Display for InvalidHtml {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(INVALID_HTML)
    }
}

impl std::error::Error for InvalidHtml {}

/// The 400 a rejected `description_html` produces.
///
/// DRF runs `validate()` before `create()`, and a `ValidationError` raised
/// there without a field name lands under `non_field_errors`
/// (`rest_framework/serializers.py:504-514`):
/// `{"non_field_errors": ["Invalid HTML passed"]}`.
#[must_use]
pub fn invalid_html_error() -> ViewError {
    let mut body = serde_json::Map::new();
    body.insert(
        "non_field_errors".to_string(),
        serde_json::Value::Array(vec![serde_json::Value::String(INVALID_HTML.to_string())]),
    );
    ViewError::Api(ApiError::ValidationError(serde_json::Value::Object(body)))
}

/// Why the normaliser did not run.
///
/// `html.fromstring` has exactly one failure mode in recover mode: a document
/// that yields no element at all (`lxml.etree.ParserError("Document is
/// empty")`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HtmlNormalizeError {
    /// `html.fromstring` raised: the input has no element.
    Empty,
}

impl std::fmt::Display for HtmlNormalizeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => f.write_str(INVALID_HTML),
        }
    }
}

impl std::error::Error for HtmlNormalizeError {}

/// `html.fromstring(value)`'s accept/reject boundary.
///
/// `Ok(())` when libxml2 would return an element, [`InvalidHtml`] when it
/// raises `ParserError("Document is empty")`.
///
/// The rule is "did the parser create a document element at all": a start tag
/// (even an unterminated one — `"<p"` parses to an empty `<body>`) or any
/// non-whitespace text is enough; whitespace, comments, processing
/// instructions and stray end tags are not. `NUL` bytes are text to libxml2
/// (it replaces them with U+FFFD), so `"\0"` is accepted.
///
/// # Errors
///
/// [`InvalidHtml`] when the input has no element.
pub fn check(value: &str) -> Result<(), InvalidHtml> {
    match parse::document(value) {
        Some(_) => Ok(()),
        None => Err(InvalidHtml),
    }
}

/// `html.tostring(html.fromstring(value), encoding="unicode")`.
///
/// # Errors
///
/// [`HtmlNormalizeError::Empty`] when libxml2 refuses the input.
pub fn normalize(value: &str) -> Result<String, HtmlNormalizeError> {
    let Some(root) = parse::document(value) else {
        return Err(HtmlNormalizeError::Empty);
    };
    let mut out = String::with_capacity(value.len() + 16);
    parse::render_fromstring(&root, value, &mut out);
    Ok(out)
}

/// The libxml2 HTML 4 parser subset `lxml.html.fromstring` runs.
///
/// Tree state mirrors `htmlParserCtxt`'s `nameTab` (a stack of open elements)
/// and its three insertion modes, because those are what decide where a node
/// lands, not the input nesting.
mod parse {
    use crate::common::html_tables::{ENTITIES, START_CLOSE};

    /// One node of the parsed document.
    pub enum Node {
        Element(Element),
        Text(String),
        Comment(String),
    }

    /// An element with its attributes in source order.
    pub struct Element {
        pub name: String,
        /// `None` for a bare attribute (`<input checked>`); lxml keeps the
        /// attribute without a value child, which serialises without `=""`.
        pub attrs: Vec<(String, Option<String>)>,
        pub children: Vec<Node>,
    }

    /// `ctxt->endCheckState`: how the current element's content is scanned.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum DataMode {
        Normal,
        /// `<textarea>`, `<title>`: references are decoded, tags are not.
        Rcdata,
        /// `<style>`, `<iframe>`, `<noembed>`, `<noframes>`, `<xmp>`.
        Rawtext,
        /// `<script>`.
        Script,
        /// `<plaintext>`: everything to EOF is text.
        Plaintext,
    }

    /// `htmlEndPriority` (`HTMLparser.c`): an end tag may only close elements
    /// of lower or equal priority (anything unlisted is 100).
    fn end_priority(name: &str) -> u32 {
        match name {
            "div" => 150,
            "td" | "th" => 160,
            "tr" => 170,
            "thead" | "tbody" | "tfoot" => 180,
            "table" => 190,
            "head" | "body" => 200,
            "html" => 220,
            _ => 100,
        }
    }

    /// The `empty` flag of `html40ElementTable`: serialised without a closing
    /// tag.
    const VOID_TAGS: [&str; 13] = [
        "area", "base", "basefont", "br", "col", "frame", "hr", "img", "input", "isindex", "link",
        "meta", "param",
    ];

    /// Attributes serialised without a value (`htmlBooleanAttrs`).
    const BOOLEAN_ATTRS: [&str; 13] = [
        "checked", "compact", "declare", "defer", "disabled", "ismap", "multiple", "nohref",
        "noresize", "noshade", "nowrap", "readonly", "selected",
    ];

    /// `lxml.html.defs.block_tags`: decides `div` vs `span` for a multi-root
    /// fragment.
    const BLOCK_TAGS: [&str; 41] = [
        "address",
        "blockquote",
        "center",
        "del",
        "div",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "hr",
        "ins",
        "isindex",
        "noscript",
        "p",
        "pre",
        "dir",
        "dl",
        "dt",
        "dd",
        "li",
        "menu",
        "ol",
        "ul",
        "table",
        "caption",
        "colgroup",
        "col",
        "thead",
        "tfoot",
        "tbody",
        "tr",
        "td",
        "th",
        "fieldset",
        "form",
        "legend",
        "optgroup",
        "option",
        "html",
    ];

    /// `htmlC1Remap`: numeric references in `0x80..=0x9F` become their
    /// Windows-1252 characters.
    const C1_REMAP: [u32; 32] = [
        0x20AC, 0x0081, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160,
        0x2039, 0x0152, 0x008D, 0x017D, 0x008F, 0x0090, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022,
        0x2013, 0x2014, 0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0x009D, 0x017E, 0x0178,
    ];

    /// The three values of `htmlInsertMode`.
    const INSERT_INITIAL: u8 = 0;
    const INSERT_IN_HEAD: u8 = 1;
    const INSERT_IN_BODY: u8 = 2;

    /// `IS_WS_HTML`: space and `\t\n\f\r` (but not `\v`).
    fn is_ws(byte: u8) -> bool {
        byte == b' ' || ((0x09..=0x0D).contains(&byte) && byte != 0x0B)
    }

    fn is_letter(byte: u8) -> bool {
        byte.is_ascii_alphabetic()
    }

    fn eq_ignore_ascii_case(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.len() >= needle.len() && haystack[..needle.len()].eq_ignore_ascii_case(needle)
    }

    /// `htmlCheckAutoClose(newtag, oldtag)`: does `newtag` imply `oldtag`'s
    /// end?
    fn implies_close(open_tag: &str, new_tag: &str) -> bool {
        START_CLOSE
            .binary_search_by(|(old, new)| {
                old.as_bytes()
                    .cmp(open_tag.as_bytes())
                    .then(new.as_bytes().cmp(new_tag.as_bytes()))
            })
            .is_ok()
    }

    fn is_void(name: &str) -> bool {
        VOID_TAGS.contains(&name)
    }

    fn is_boolean_attr(name: &str) -> bool {
        BOOLEAN_ATTRS
            .iter()
            .any(|attr| name.eq_ignore_ascii_case(attr))
    }

    /// `IS_BLANK_CH`, used to trim a URI attribute before escaping.
    fn is_blank_ch(ch: char) -> bool {
        ch == ' ' || ('\t'..='\r').contains(&ch) || ch == '\u{b}'
    }

    struct Parser<'a> {
        src: &'a str,
        bytes: &'a [u8],
        pos: usize,
        /// Open elements, outermost first. `stack[0]` is `<html>` once it
        /// exists.
        stack: Vec<Element>,
        /// `ctxt->html`.
        insert_mode: u8,
        /// `ctxt->endCheckState`.
        data_mode: DataMode,
        /// The `<html>` element once it is closed (libxml2 pops it at EOF but
        /// the document still owns it).
        root: Option<Element>,
    }

    /// Parse `input` the way `lxml.html.document_fromstring` does.
    ///
    /// `None` when libxml2 produced no document element at all (the
    /// `ParserError("Document is empty")` case).
    pub fn document(input: &str) -> Option<Element> {
        let mut parser = Parser {
            src: input,
            bytes: input.as_bytes(),
            pos: 0,
            stack: Vec::new(),
            insert_mode: INSERT_INITIAL,
            data_mode: DataMode::Normal,
            root: None,
        };
        parser.parse_document();
        parser.root
    }

    impl Parser<'_> {
        fn cur(&self) -> Option<u8> {
            self.bytes.get(self.pos).copied()
        }

        fn at(&self, offset: usize) -> Option<u8> {
            self.bytes.get(self.pos + offset).copied()
        }

        fn skip_blanks(&mut self) {
            while self.cur().is_some_and(is_ws) {
                self.pos += 1;
            }
        }

        /// `htmlParseDocument`'s prologue: leading blanks, comments and
        /// processing instructions before the first element, then an optional
        /// doctype (`HTMLparser.c:4428-4500`).
        fn parse_document(&mut self) {
            self.skip_blanks();
            loop {
                if self.cur() == Some(b'<')
                    && self.at(1) == Some(b'!')
                    && self.at(2) == Some(b'-')
                    && self.at(3) == Some(b'-')
                {
                    self.parse_comment();
                    self.skip_blanks();
                    continue;
                }
                if self.cur() == Some(b'<') && self.at(1) == Some(b'?') {
                    self.pos += 1;
                    self.parse_bogus_comment();
                    self.skip_blanks();
                    continue;
                }
                break;
            }
            if eq_ignore_ascii_case(&self.bytes[self.pos..], b"<!doctype") {
                self.skip_doctype();
            }
            self.skip_blanks();
            loop {
                if self.cur() == Some(b'<')
                    && self.at(1) == Some(b'!')
                    && self.at(2) == Some(b'-')
                    && self.at(3) == Some(b'-')
                {
                    self.parse_comment();
                    self.skip_blanks();
                    continue;
                }
                if self.cur() == Some(b'<') && self.at(1) == Some(b'?') {
                    self.pos += 1;
                    self.parse_bogus_comment();
                    self.skip_blanks();
                    continue;
                }
                break;
            }
            self.parse_content();
        }

        /// `htmlParseContent`: the element loop, including the data modes.
        fn parse_content(&mut self) {
            while self.pos < self.bytes.len() {
                if self.data_mode != DataMode::Normal {
                    self.parse_raw_text();
                    continue;
                }
                if self.cur() != Some(b'<') {
                    self.parse_char_data();
                    continue;
                }
                match self.at(1) {
                    Some(b'/') => self.parse_end_tag(),
                    Some(b'!') => {
                        if self.at(2) == Some(b'-') && self.at(3) == Some(b'-') {
                            self.parse_comment();
                        } else if eq_ignore_ascii_case(&self.bytes[self.pos..], b"<!doctype") {
                            self.skip_doctype();
                        } else {
                            self.pos += 2;
                            self.parse_bogus_comment();
                        }
                    }
                    Some(b'?') => {
                        // `SKIP(1)` then a bogus comment: the `?` is content.
                        self.pos += 1;
                        self.parse_bogus_comment();
                    }
                    Some(byte) if is_letter(byte) => self.parse_start_tag(),
                    _ => {
                        self.start_char_data();
                        self.push_text("<");
                        self.pos += 1;
                    }
                }
            }
            // `htmlAutoCloseOnEnd`: close what the input left open.
            while !self.stack.is_empty() {
                self.pop_element();
            }
        }

        /// `htmlStartCharData` + `htmlCheckImplied`.
        fn start_char_data(&mut self) {
            if self
                .stack
                .last()
                .is_some_and(|element| element.name == "head")
            {
                self.auto_close("p");
            }
            self.check_implied("p");
        }

        /// `htmlCheckImplied` (`HTMLparser.c:1210-1275`): create the `html`,
        /// `head` and `body` wrappers libxml2 invents.
        fn check_implied(&mut self, new_tag: &str) {
            if new_tag == "html" {
                return;
            }
            if self.stack.is_empty() {
                self.push_element("html", Vec::new());
            }
            if new_tag == "body" || new_tag == "head" {
                return;
            }
            let is_head_tag = matches!(
                new_tag,
                "script" | "style" | "meta" | "link" | "title" | "base"
            );
            if self.stack.len() <= 1 && is_head_tag {
                if self.insert_mode >= INSERT_IN_HEAD {
                    return;
                }
                self.push_element("head", Vec::new());
            } else if !matches!(new_tag, "noframes" | "frame" | "frameset") {
                if self.insert_mode >= INSERT_IN_BODY {
                    return;
                }
                if self
                    .stack
                    .iter()
                    .any(|element| element.name == "body" || element.name == "head")
                {
                    return;
                }
                self.push_element("body", Vec::new());
            }
        }

        fn push_element(&mut self, name: &str, attrs: Vec<(String, Option<String>)>) {
            if name == "head" && self.insert_mode < INSERT_IN_HEAD {
                self.insert_mode = INSERT_IN_HEAD;
            }
            if name == "body" && self.insert_mode < INSERT_IN_BODY {
                self.insert_mode = INSERT_IN_BODY;
            }
            self.stack.push(Element {
                name: name.to_string(),
                attrs,
                children: Vec::new(),
            });
        }

        /// `htmlnamePop` + the SAX `endElement`: the closed element becomes a
        /// child of its parent, or the document root when it has none.
        fn pop_element(&mut self) {
            let Some(element) = self.stack.pop() else {
                return;
            };
            match self.stack.last_mut() {
                Some(parent) => parent.children.push(Node::Element(element)),
                None => {
                    if self.root.is_none() {
                        self.root = Some(element);
                    }
                }
            }
        }

        fn attach(&mut self, node: Node) {
            if let Some(parent) = self.stack.last_mut() {
                parent.children.push(node);
            }
        }

        fn push_text(&mut self, text: &str) {
            if text.is_empty() {
                return;
            }
            if let Some(Node::Text(existing)) = self
                .stack
                .last_mut()
                .and_then(|parent| parent.children.last_mut())
            {
                existing.push_str(text);
                return;
            }
            self.attach(Node::Text(text.to_string()));
        }

        fn push_comment(&mut self, text: &str) {
            self.attach(Node::Comment(text.to_string()));
        }

        /// `htmlAutoClose`: pop while the new tag implies the top element's
        /// end.
        fn auto_close(&mut self, new_tag: &str) {
            while let Some(top) = self.stack.last() {
                if !implies_close(&top.name, new_tag) {
                    break;
                }
                self.pop_element();
            }
        }

        /// `htmlAutoCloseOnClose`: an end tag only pops while the priorities
        /// allow it, and only when a matching open element exists.
        fn auto_close_on_close(&mut self, new_tag: &str) {
            let priority = end_priority(new_tag);
            let mut matching = None;
            for index in (0..self.stack.len()).rev() {
                if self.stack[index].name == new_tag {
                    matching = Some(index);
                    break;
                }
                if end_priority(&self.stack[index].name) > priority {
                    return;
                }
            }
            let Some(matching) = matching else {
                return;
            };
            // Leaves the matching element itself on top: `htmlParseEndTag`
            // pops it after `htmlAutoCloseOnClose` returns.
            while self.stack.len() > matching + 1 {
                self.pop_element();
            }
        }

        /// `htmlParseHTMLName`: lowercase ASCII, stop at `/`, `>`, `=` or
        /// whitespace after the first byte.
        fn parse_name(&mut self) -> String {
            let start = self.pos;
            let mut index = self.pos;
            while index < self.bytes.len() {
                let byte = self.bytes[index];
                if index != start && (byte == b'/' || byte == b'>' || byte == b'=' || is_ws(byte)) {
                    break;
                }
                index += 1;
            }
            self.pos = index;
            let mut name = String::with_capacity(index - start);
            for &byte in &self.bytes[start..index] {
                if byte == 0 {
                    name.push('\u{FFFD}');
                } else {
                    name.push(byte.to_ascii_lowercase() as char);
                }
            }
            name
        }

        /// `htmlParseStartTag` + `htmlParseElementInternal`.
        fn parse_start_tag(&mut self) {
            self.pos += 1;
            let name = self.parse_name();
            if name.is_empty() {
                return;
            }
            self.auto_close(&name);
            self.check_implied(&name);

            let mut discard = false;
            if !self.stack.is_empty() && name == "html" {
                discard = true;
            }
            if self.stack.len() != 1 && name == "head" {
                discard = true;
            }
            if name == "body" && self.stack.iter().any(|element| element.name == "body") {
                discard = true;
            }

            let mut attrs: Vec<(String, Option<String>)> = Vec::new();
            self.skip_blanks();
            loop {
                let Some(byte) = self.cur() else {
                    discard = true;
                    break;
                };
                if byte == b'>' {
                    break;
                }
                if byte == b'/' && self.at(1) == Some(b'>') {
                    break;
                }
                if byte == b'/' {
                    self.pos += 1;
                    self.skip_blanks();
                    continue;
                }
                let attr_name = self.parse_name();
                if attr_name.is_empty() {
                    // `htmlParseAttribute` returns nothing: consume one byte
                    // so the loop always advances.
                    self.pos += 1;
                    continue;
                }
                self.skip_blanks();
                let mut value = None;
                if self.cur() == Some(b'=') {
                    self.pos += 1;
                    self.skip_blanks();
                    value = Some(self.parse_attr_value());
                }
                // `htmlAttrHashInsert`: the first occurrence wins.
                if !attrs.iter().any(|(existing, _)| *existing == attr_name) {
                    attrs.push((attr_name, value));
                }
                self.skip_blanks();
            }
            if self.pos >= self.bytes.len() {
                discard = true;
            }
            if discard {
                // `htmlParseElementInternal` consumes the tag's `>` (or `/>`)
                // even when `htmlParseStartTag` ignored the element, so the
                // `>` never becomes text.
                if self.cur() == Some(b'/') && self.at(1) == Some(b'>') {
                    self.pos += 2;
                } else if self.cur() == Some(b'>') {
                    self.pos += 1;
                }
                return;
            }
            self.push_element(&name, attrs);

            if self.cur() == Some(b'/') && self.at(1) == Some(b'>') {
                self.pos += 2;
                self.pop_element();
                return;
            }
            if self.cur() != Some(b'>') {
                return;
            }
            self.pos += 1;
            if is_void(&name) {
                self.pop_element();
                return;
            }
            self.data_mode = match name.as_str() {
                "script" => DataMode::Script,
                "style" | "iframe" | "noembed" | "noframes" | "xmp" => DataMode::Rawtext,
                "textarea" | "title" => DataMode::Rcdata,
                "plaintext" => DataMode::Plaintext,
                _ => DataMode::Normal,
            };
        }

        /// `htmlParseAttValue`: a quoted or unquoted value with references
        /// decoded.
        fn parse_attr_value(&mut self) -> String {
            let mut out = String::new();
            let mut quote = None;
            if let Some(byte) = self.cur()
                && (byte == b'"' || byte == b'\'')
            {
                quote = Some(byte);
                self.pos += 1;
            }
            while let Some(byte) = self.cur() {
                match quote {
                    Some(quote_byte) => {
                        if byte == quote_byte {
                            self.pos += 1;
                            break;
                        }
                    }
                    None => {
                        if is_ws(byte) || byte == b'>' {
                            break;
                        }
                    }
                }
                if byte == b'&'
                    && let Some((text, consumed)) = self.entity_at(self.pos, true)
                {
                    out.push_str(&text);
                    self.pos += consumed;
                    continue;
                }
                if byte == 0 {
                    out.push('\u{FFFD}');
                    self.pos += 1;
                    continue;
                }
                if byte == b'\r' {
                    out.push('\n');
                    self.pos += if self.at(1) == Some(b'\n') { 2 } else { 1 };
                    continue;
                }
                let ch = self.src[self.pos..].chars().next().unwrap_or('\u{FFFD}');
                out.push(ch);
                self.pos += ch.len_utf8();
            }
            out
        }

        /// `htmlParseCharData` in `mode == 0`: text up to `<`.
        ///
        /// `htmlCharDataSAXCallback` (`HTMLparser.c:2942-2975`) keeps the
        /// leading blanks of a run in `<html>`/`<head>` and only then runs
        /// `htmlStartCharData`, which creates the implied `<body>`; a
        /// whitespace-only run creates nothing.
        fn parse_char_data(&mut self) {
            let mut out = String::new();
            while let Some(byte) = self.cur() {
                match byte {
                    b'<' => break,
                    b'&' => {
                        if let Some((text, consumed)) = self.entity_at(self.pos, false) {
                            out.push_str(&text);
                            self.pos += consumed;
                        } else {
                            out.push('&');
                            self.pos += 1;
                        }
                    }
                    0 => {
                        out.push('\u{FFFD}');
                        self.pos += 1;
                    }
                    b'\r' => {
                        out.push('\n');
                        self.pos += if self.at(1) == Some(b'\n') { 2 } else { 1 };
                    }
                    _ => {
                        let ch = self.src[self.pos..].chars().next().unwrap_or('\u{FFFD}');
                        out.push(ch);
                        self.pos += ch.len_utf8();
                    }
                }
            }
            let before_body = self
                .stack
                .last()
                .is_none_or(|element| element.name == "html" || element.name == "head");
            if !before_body {
                self.push_text(&out);
                return;
            }
            let lead = out.find(|ch: char| !is_ws(ch as u8)).unwrap_or(out.len());
            if lead > 0 {
                let blanks = out[..lead].to_string();
                self.push_text(&blanks);
            }
            if lead == out.len() {
                return;
            }
            self.start_char_data();
            let rest = out[lead..].to_string();
            self.push_text(&rest);
        }

        /// The `DATA_SCRIPT` / `DATA_RAWTEXT` / `DATA_RCDATA` / `DATA_PLAINTEXT`
        /// scan: text up to the element's own end tag.
        fn parse_raw_text(&mut self) {
            let Some(name) = self.stack.last().map(|element| element.name.clone()) else {
                self.data_mode = DataMode::Normal;
                return;
            };
            if self.data_mode == DataMode::Plaintext {
                let rest = &self.src[self.pos..];
                self.pos = self.bytes.len();
                self.push_text(rest);
                return;
            }
            let start = self.pos;
            while let Some(offset) = self.src[self.pos..].find('<') {
                let candidate = self.pos + offset;
                let after = candidate + 1;
                let matches_name = self.bytes.get(after) == Some(&b'/')
                    && candidate + 2 + name.len() <= self.bytes.len()
                    && self.bytes[after + 1..after + 1 + name.len()]
                        .eq_ignore_ascii_case(name.as_bytes())
                    && self
                        .bytes
                        .get(after + 1 + name.len())
                        .is_some_and(|byte| *byte == b'>' || *byte == b'/' || is_ws(*byte));
                if matches_name {
                    let text = &self.src[start..candidate];
                    self.write_raw_text(text);
                    self.pos = candidate;
                    self.parse_end_tag();
                    self.data_mode = DataMode::Normal;
                    return;
                }
                self.pos = candidate + 1;
            }
            let text = &self.src[start..];
            self.pos = self.bytes.len();
            self.write_raw_text(text);
        }

        fn write_raw_text(&mut self, text: &str) {
            if self.data_mode == DataMode::Rcdata {
                let decoded = decode_text(text);
                self.push_text(&decoded);
            } else {
                let text = text.replace('\0', "\u{FFFD}");
                self.push_text(&text);
            }
        }

        /// `htmlParseEndTag`.
        fn parse_end_tag(&mut self) {
            self.pos += 2;
            let Some(byte) = self.cur() else {
                self.start_char_data();
                self.push_text("</");
                return;
            };
            if byte == b'>' {
                self.pos += 1;
                return;
            }
            if !is_letter(byte) {
                self.parse_bogus_comment();
                return;
            }
            let name = self.parse_name();
            self.skip_blanks();
            loop {
                let Some(byte) = self.cur() else {
                    return;
                };
                if byte == b'>' || (byte == b'/' && self.at(1) == Some(b'>')) {
                    break;
                }
                if byte == b'/' {
                    self.pos += 1;
                    self.skip_blanks();
                    continue;
                }
                let attr_name = self.parse_name();
                if attr_name.is_empty() {
                    self.pos += 1;
                    continue;
                }
                self.skip_blanks();
                if self.cur() == Some(b'=') {
                    self.pos += 1;
                    self.skip_blanks();
                    self.parse_attr_value();
                }
                self.skip_blanks();
            }
            if self.cur() == Some(b'>') {
                self.pos += 1;
            } else if self.cur() == Some(b'/') && self.at(1) == Some(b'>') {
                self.pos += 2;
            } else {
                return;
            }
            if self.stack.is_empty() || !self.stack.iter().any(|element| element.name == name) {
                return;
            }
            self.auto_close_on_close(&name);
            if self
                .stack
                .last()
                .is_some_and(|element| element.name == name)
            {
                self.pop_element();
            }
        }

        /// `htmlParseComment(bogus = 0)`: the content is everything up to the
        /// first `-->` (or the `--!>` form libxml2 also accepts).
        fn parse_comment(&mut self) {
            self.pos += 4;
            if self.cur() == Some(b'>') {
                self.pos += 1;
                self.push_comment("");
                return;
            }
            if self.cur() == Some(b'-') && self.at(1) == Some(b'>') {
                self.pos += 2;
                self.push_comment("");
                return;
            }
            let start = self.pos;
            let mut index = self.pos;
            while index + 2 < self.bytes.len() {
                if self.bytes[index] == b'-' && self.bytes[index + 1] == b'-' {
                    if self.bytes[index + 2] == b'>' {
                        let content = &self.src[start..index];
                        self.pos = index + 3;
                        self.push_comment(content);
                        return;
                    }
                    if index + 3 < self.bytes.len()
                        && self.bytes[index + 2] == b'!'
                        && self.bytes[index + 3] == b'>'
                    {
                        let content = &self.src[start..index];
                        self.pos = index + 4;
                        self.push_comment(content);
                        return;
                    }
                }
                index += 1;
            }
            let content = &self.src[start..];
            self.pos = self.bytes.len();
            self.push_comment(content);
        }

        /// `htmlParseComment(bogus = 1)`: content up to `>`.
        fn parse_bogus_comment(&mut self) {
            let start = self.pos;
            match self.src[self.pos..].find('>') {
                Some(offset) => {
                    let content = &self.src[start..start + offset];
                    self.pos = start + offset + 1;
                    self.push_comment(content);
                }
                None => {
                    let content = &self.src[start..];
                    self.pos = self.bytes.len();
                    self.push_comment(content);
                }
            }
        }

        /// `htmlParseDocTypeDecl`: the declared subset never reaches the tree
        /// (`tostring` writes no doctype), so only its extent matters.
        fn skip_doctype(&mut self) {
            self.pos += 9;
            let mut quote = None;
            while let Some(byte) = self.cur() {
                self.pos += 1;
                match quote {
                    Some(quote_byte) => {
                        if byte == quote_byte {
                            quote = None;
                        }
                    }
                    None => {
                        if byte == b'"' || byte == b'\'' {
                            quote = Some(byte);
                        } else if byte == b'>' {
                            return;
                        }
                    }
                }
            }
        }

        /// `htmlParseReference`: a numeric or named reference at `pos` (which
        /// points at `&`). Returns the replacement text and the bytes to
        /// consume.
        fn entity_at(&self, pos: usize, is_attr: bool) -> Option<(String, usize)> {
            let rest = &self.bytes[pos + 1..];
            if rest.first() == Some(&b'#') {
                let mut index = 1;
                let hex = matches!(rest.get(1), Some(b'x') | Some(b'X'));
                if hex {
                    index = 2;
                }
                let digits_start = index;
                while index < rest.len()
                    && if hex {
                        rest[index].is_ascii_hexdigit()
                    } else {
                        rest[index].is_ascii_digit()
                    }
                {
                    index += 1;
                }
                if index == digits_start {
                    return None;
                }
                let base = if hex { 16 } else { 10 };
                let mut code: u32 = 0;
                for &digit in &rest[digits_start..index] {
                    let value = (digit as char).to_digit(base).unwrap_or(0);
                    code = code.saturating_mul(base).saturating_add(value);
                    if code >= 0x11_0000 {
                        code = 0x11_0000;
                    }
                }
                let mut consumed = 1 + index;
                if rest.get(index) == Some(&b';') {
                    consumed += 1;
                }
                return Some((codepoint_to_char(code).to_string(), consumed));
            }
            if !rest.first().is_some_and(|byte| is_letter(*byte)) {
                return None;
            }
            let mut best: Option<(&str, &str, bool, usize)> = None;
            for &(name, replacement, semicolon) in ENTITIES {
                if !rest.starts_with(name.as_bytes()) {
                    continue;
                }
                let terminator = rest.get(name.len()).copied();
                let terminated = match terminator {
                    Some(b';') => true,
                    _ if !semicolon => {
                        if is_attr {
                            !terminator
                                .is_some_and(|byte| byte.is_ascii_alphanumeric() || byte == b'=')
                        } else {
                            true
                        }
                    }
                    _ => false,
                };
                if !terminated {
                    continue;
                }
                if best.is_none_or(|(best_name, ..)| name.len() > best_name.len()) {
                    best = Some((name, replacement, semicolon, name.len()));
                }
            }
            let (name, replacement, _, _) = best?;
            let consumed = 1 + name.len() + usize::from(rest.get(name.len()) == Some(&b';'));
            Some((replacement.to_string(), consumed))
        }
    }

    /// `htmlCodePointToUtf8`: `0`, surrogates and out-of-range code points
    /// become U+FFFD; `0x80..=0x9F` are remapped to Windows-1252.
    fn codepoint_to_char(code: u32) -> char {
        if (0x80..0xA0).contains(&code) {
            return char::from_u32(C1_REMAP[(code - 0x80) as usize]).unwrap_or('\u{FFFD}');
        }
        if code == 0 || (0xD800..0xE000).contains(&code) || code > 0x10_FFFF {
            return '\u{FFFD}';
        }
        char::from_u32(code).unwrap_or('\u{FFFD}')
    }

    /// Decode the references of an `RCDATA` text run (used by
    /// [`Parser::write_raw_text`]).
    fn decode_text(text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut index = 0;
        while index < text.len() {
            let rest = &text[index..];
            let byte = rest.as_bytes()[0];
            if byte == b'&' {
                let scratch = Parser {
                    src: text,
                    bytes: text.as_bytes(),
                    pos: index,
                    stack: Vec::new(),
                    insert_mode: INSERT_INITIAL,
                    data_mode: DataMode::Normal,
                    root: None,
                };
                if let Some((decoded, consumed)) = scratch.entity_at(index, false) {
                    out.push_str(&decoded);
                    index += consumed;
                    continue;
                }
            }
            if byte == b'\r' {
                out.push('\n');
                index += if rest.as_bytes().get(1) == Some(&b'\n') {
                    2
                } else {
                    1
                };
                continue;
            }
            let ch = rest.chars().next().unwrap_or('\u{FFFD}');
            out.push(if byte == 0 { '\u{FFFD}' } else { ch });
            index += ch.len_utf8();
        }
        out
    }

    /// The `lxml.html.fromstring` reshaping plus `html.tostring`.
    pub fn render_fromstring(root: &Element, input: &str, out: &mut String) {
        if looks_like_full_html(input) {
            write_element(out, root, None, false);
            return;
        }
        // `heads = doc.findall('head')` wins over the body reshaping: a
        // document that declared a head is returned whole
        // (`lxml/html/__init__.py:876-884`).
        if root
            .children
            .iter()
            .any(|node| matches!(node, Node::Element(element) if element.name == "head"))
        {
            write_element(out, root, None, false);
            return;
        }
        let Some(body) = root
            .children
            .iter()
            .position(|node| matches!(node, Node::Element(element) if element.name == "body"))
        else {
            write_element(out, root, None, false);
            return;
        };
        let Node::Element(body_element) = &root.children[body] else {
            unreachable!("the body child is an element");
        };
        let element_children = body_element
            .children
            .iter()
            .filter(|node| !matches!(node, Node::Text(_)))
            .count();
        let leading = match body_element.children.first() {
            Some(Node::Text(text)) => text.as_str(),
            _ => "",
        };
        let trailing = match body_element.children.last() {
            Some(Node::Text(text)) => text.as_str(),
            _ => "",
        };
        if element_children == 1 && is_blank(leading) && is_blank(trailing) {
            // Return the single child, with the blank tail it had in the body.
            let index = body_element
                .children
                .iter()
                .position(|node| !matches!(node, Node::Text(_)))
                .expect("one element child");
            write_node(out, &body_element.children, index, Some("body"), None);
            return;
        }
        let renamed = if contains_block_tag(body_element) {
            "div"
        } else {
            "span"
        };
        // The body is serialised in place, so the text after it is its tail.
        write_node(out, &root.children, body, None, Some(renamed));
    }

    /// `^\s*<(?:html|!doctype)` (`lxml/html/__init__.py:731-734`).
    fn looks_like_full_html(input: &str) -> bool {
        let trimmed = input.trim_start();
        let bytes = trimmed.as_bytes();
        if bytes.first() != Some(&b'<') {
            return false;
        }
        let rest = &bytes[1..];
        rest.len() >= 4 && rest[..4].eq_ignore_ascii_case(b"html")
            || rest.len() >= 8 && rest[..8].eq_ignore_ascii_case(b"!doctype")
    }

    /// Python's `str.strip()` blankness, which is what the single-element
    /// branch tests (`str.isspace` covers `\x1c`–`\x1f` too).
    fn is_blank(text: &str) -> bool {
        text.chars()
            .all(|ch| ch.is_whitespace() || matches!(ch, '\u{1c}'..='\u{1f}') || ch == '\u{85}')
    }

    fn contains_block_tag(element: &Element) -> bool {
        if BLOCK_TAGS.contains(&element.name.as_str()) {
            return true;
        }
        element.children.iter().any(|node| match node {
            Node::Element(child) => contains_block_tag(child),
            _ => false,
        })
    }

    fn write_node(
        out: &mut String,
        nodes: &[Node],
        index: usize,
        parent: Option<&str>,
        rename: Option<&str>,
    ) {
        match &nodes[index] {
            Node::Element(element) => write_element(out, element, rename, false),
            Node::Comment(content) => {
                out.push_str("<!--");
                out.push_str(content);
                out.push_str("-->");
            }
            Node::Text(text) => write_text(out, text, parent),
        }
        // `with_tail`: text nodes that follow are the element's tail.
        for node in &nodes[index + 1..] {
            if let Node::Text(text) = node {
                write_text(out, text, parent);
            } else {
                break;
            }
        }
    }

    /// `xmlEncodeEntitiesReentrant` in HTML mode: `&`, `<` and `>` escaped,
    /// everything else literal. `<script>`/`<style>` content is written raw
    /// (`HTMLtree.c:780-800`).
    fn write_text(out: &mut String, text: &str, parent: Option<&str>) {
        if parent.is_some_and(|name| {
            name.eq_ignore_ascii_case("script") || name.eq_ignore_ascii_case("style")
        }) {
            out.push_str(text);
            return;
        }
        for ch in text.chars() {
            match ch {
                '&' => out.push_str("&amp;"),
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                _ => out.push(ch),
            }
        }
    }

    fn write_element(out: &mut String, element: &Element, rename: Option<&str>, _tail: bool) {
        let name = rename.unwrap_or(&element.name);
        out.push('<');
        out.push_str(name);
        for (attr_name, value) in &element.attrs {
            write_attr(out, name, attr_name, value.as_deref());
        }
        if is_void(name) {
            out.push('>');
            return;
        }
        if element.children.is_empty() {
            // `htmlNodeDumpFormatOutput` writes no closing tag for an empty
            // element whose `saveEndTag` is set: only `li` among the
            // non-empty tags.
            if name == "li" {
                out.push('>');
            } else {
                out.push_str("></");
                out.push_str(name);
                out.push('>');
            }
            return;
        }
        out.push('>');
        for node in &element.children {
            match node {
                Node::Element(child) => write_element(out, child, None, true),
                Node::Comment(content) => {
                    out.push_str("<!--");
                    out.push_str(content);
                    out.push_str("-->");
                }
                Node::Text(text) => write_text(out, text, Some(name)),
            }
        }
        out.push_str("</");
        out.push_str(name);
        out.push('>');
    }

    /// `htmlAttrDumpOutput`: name, `=`, then a quoted value.
    fn write_attr(out: &mut String, element: &str, name: &str, value: Option<&str>) {
        out.push(' ');
        out.push_str(name);
        let Some(value) = value else {
            // A bare attribute: the parser stored no value child.
            return;
        };
        if is_boolean_attr(name) {
            return;
        }
        let escaped = if is_uri_attr(element, name) {
            uri_escape(value)
        } else {
            value.to_string()
        };
        out.push('=');
        // `xmlOutputBufferWriteQuotedString`: prefer `"`, fall back to `'`
        // when the value contains `"`, and escape `"` when it contains both.
        if escaped.contains('"') && escaped.contains('\'') {
            out.push('"');
            write_attr_value(out, &escaped, true);
            out.push('"');
        } else if escaped.contains('"') {
            out.push('\'');
            write_attr_value(out, &escaped, false);
            out.push('\'');
        } else {
            out.push('"');
            write_attr_value(out, &escaped, false);
            out.push('"');
        }
    }

    fn write_attr_value(out: &mut String, value: &str, escape_quote: bool) {
        for ch in value.chars() {
            match ch {
                '&' => out.push_str("&amp;"),
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                '"' if escape_quote => out.push_str("&quot;"),
                _ => out.push(ch),
            }
        }
    }

    /// `href`/`action`/`src`, or `name` on `<a>` (`HTMLtree.c:622-628`).
    fn is_uri_attr(element: &str, name: &str) -> bool {
        name.eq_ignore_ascii_case("href")
            || name.eq_ignore_ascii_case("action")
            || name.eq_ignore_ascii_case("src")
            || (name.eq_ignore_ascii_case("name") && element.eq_ignore_ascii_case("a"))
    }

    /// `xmlURIEscapeStr` with the attribute's keep list: leading blanks are
    /// dropped, then space, control and non-ASCII bytes are percent-encoded
    /// (`HTMLtree.c:632-660`).
    fn uri_escape(value: &str) -> String {
        let trimmed = value.trim_start_matches(is_blank_ch);
        let mut out = String::with_capacity(trimmed.len());
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        for &byte in trimmed.as_bytes() {
            if byte <= 0x20 || byte >= 0x7F {
                out.push('%');
                out.push(HEX[(byte >> 4) as usize] as char);
                out.push(HEX[(byte & 0x0F) as usize] as char);
            } else {
                out.push(byte as char);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every verdict below was recorded from the real `lxml` in
    /// `apps/api/.venv` (`lxml.etree.ParserError: Document is empty`); see
    /// `tests/golden/lxml_html.jsonl`.
    #[test]
    fn the_gate_matches_lxml() {
        // The cases libxml2 refuses: no element comes out of the parser.
        for rejected in [
            "",
            "\n\t ",
            "  ",
            "<!-- only comment -->",
            "<?xml version='1.0'?>",
            "</p>",
        ] {
            assert_eq!(check(rejected), Err(InvalidHtml), "{rejected:?}");
        }
        // The cases libxml2 accepts, including the surprising ones.
        for accepted in [
            "hello",
            "\0",
            "\0a",
            "<",
            "<>",
            "< p>",
            "<p>",
            "<p",
            "<p ",
            "&bogus;",
            "<p> </p>",
            "+",
            "<!-- c --><p>x</p>",
            "<p>x</p><!-- c -->",
            "<p>x</p></p>",
            "text<p>x</p>",
        ] {
            assert_eq!(check(accepted), Ok(()), "{accepted:?}");
        }
    }

    /// Inputs whose normalisation was recorded from lxml 6.0.2 / libxml2
    /// 2.14.6 (the pins in `apps/api`) and is not in the corpus.
    #[test]
    fn normalize_matches_recorded_lxml_output() {
        let cases = [
            // Implied end tags and wrapping.
            ("<p>a<p>b<p>c", "<div><p>a</p><p>b</p><p>c</p></div>"),
            ("<p>a<h1>b</h1></p>", "<div><p>a</p><h1>b</h1></div>"),
            ("<h1><p>a</p></h1>", "<div><h1></h1><p>a</p></div>"),
            ("<b>x<p>y</p></b>", "<div><b>x</b><p>y</p></div>"),
            ("<span><p>a</p></span>", "<span><p>a</p></span>"),
            ("<div><p>a</div>", "<div><p>a</p></div>"),
            ("<b><i>x</b></i>", "<b><i>x</i></b>"),
            (
                "<table><tr><td>a<td>b<tr><td>c</table>",
                "<table><tr><td>a</td><td>b</td></tr><tr><td>c</td></tr></table>",
            ),
            (
                "<dl><dt>a<dd>b<dt>c</dl>",
                "<dl><dt>a</dt><dd>b</dd><dt>c</dt></dl>",
            ),
            (
                "<select><option>a<option>b</select>",
                "<select><option>a</option><option>b</option></select>",
            ),
            (
                "<a href=x><a href=y>z</a>",
                "<span><a href=\"x\"></a><a href=\"y\">z</a></span>",
            ),
            ("<p>a</p></p>", "<p>a</p>"),
            ("<p>x</p>\n", "<p>x</p>\n"),
            ("\n<p>x</p>", "<p>x</p>"),
            ("  <p>x</p>  ", "<p>x</p>  "),
            ("<p/>x", "<div><p></p>x</div>"),
            ("<p />", "<p></p>"),
            ("<br />", "<br>"),
            ("<img src=\"x\"/>", "<img src=\"x\">"),
            ("<li></li>", "<li>"),
            ("<td></td>", "<td></td>"),
            // Attributes.
            ("<p foo>x</p>", "<p foo>x</p>"),
            (
                "<input type='checkbox' checked>",
                "<input type=\"checkbox\" checked>",
            ),
            ("<p title=x>y</p>", "<p title=\"x\">y</p>"),
            ("<p title=a title=b>x</p>", "<p title=\"a\">x</p>"),
            (
                "<P TITLE=x CLASS=y>z</P>",
                "<p title=\"x\" class=\"y\">z</p>",
            ),
            ("<a href=\" a b \">x</a>", "<a href=\"a%20b%20\">x</a>"),
            ("<img src=\"\u{fc}.png\">", "<img src=\"%C3%BC.png\">"),
            (
                "<p title=\"a&quot;b'c\">x</p>",
                "<p title=\"a&quot;b'c\">x</p>",
            ),
            (
                "<p title=\"a&amp;b&lt;c&gt;d\">x</p>",
                "<p title=\"a&amp;b&lt;c&gt;d\">x</p>",
            ),
            ("<p title=\"&#13;\">x</p>", "<p title=\"\r\">x</p>"),
            // Text, entities and newlines.
            ("<p>a\r\nb</p>", "<p>a\nb</p>"),
            (
                "<p>&amp &#38; &ampx; &AMP; &Amp;</p>",
                "<p>&amp; &amp; &amp;x; &amp; &amp;Amp;</p>",
            ),
            (
                "<p>&#x110000; &#xD800; &#65</p>",
                "<p>\u{FFFD} \u{FFFD} A</p>",
            ),
            (
                "<p>&notin; &not; &hellip;</p>",
                "<p>\u{2209} \u{ac} \u{2026}</p>",
            ),
            // Comments, doctypes and full documents.
            ("<p>a</p><!--x-->b", "<div><p>a</p><!--x-->b</div>"),
            (
                "<html><head><title>t</title></head><body>x</body></html>",
                "<html><head><title>t</title></head><body>x</body></html>",
            ),
            (
                "<!DOCTYPE html><html><body><p>x</p></body></html>",
                "<html><body><p>x</p></body></html>",
            ),
            (
                "<head><title>x</title></head>y",
                "<html><head><title>x</title></head><body>y</body></html>",
            ),
            ("<body>text</body>", "<span>text</span>"),
            ("<!-- c --><p>x</p>", "<p>x</p>"),
            // Raw text elements.
            (
                "<script>a<b && c>d</script>",
                "<html><head><script>a<b && c>d</script></head></html>",
            ),
            (
                "<style>a{b:<c>}</style>",
                "<html><head><style>a{b:<c>}</style></head></html>",
            ),
            ("<textarea>a<b</textarea>", "<textarea>a&lt;b</textarea>"),
            (
                "<textarea>a &amp; b</textarea>",
                "<textarea>a &amp; b</textarea>",
            ),
            (
                "<title>a &amp; b</title>",
                "<html><head><title>a &amp; b</title></head></html>",
            ),
            // Unparseable ends.
            ("<p", "<span></span>"),
            ("<p class=\"x", "<span></span>"),
            ("<", "<span>&lt;</span>"),
            ("<>", "<span>&lt;&gt;</span>"),
            ("< p>", "<span>&lt; p&gt;</span>"),
            ("&bogus;", "<span>&amp;bogus;</span>"),
            ("\0a", "<span>\u{FFFD}a</span>"),
        ];
        let mut failures = Vec::new();
        for (input, expected) in cases {
            match normalize(input) {
                Ok(actual) if actual == expected => {}
                Ok(actual) => {
                    failures.push(format!("{input:?}: got {actual:?}, expected {expected:?}"))
                }
                Err(error) => failures.push(format!("{input:?}: {error}")),
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn the_rejection_is_drfs_non_field_error() {
        let error = invalid_html_error();
        let ViewError::Api(api) = error else {
            panic!("expected an API error");
        };
        assert_eq!(api.status(), 400);
        assert_eq!(
            serde_json::to_string(&match api.render() {
                Ok(response) => String::from_utf8(response.body).expect("utf8"),
                Err(error) => panic!("render: {error}"),
            })
            .expect("json"),
            r#""{\"non_field_errors\":[\"Invalid HTML passed\"]}""#
        );
    }
}
