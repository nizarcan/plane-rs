// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `str(BeautifulSoup(html, "html.parser"))`, byte for byte.
//!
//! `plane/bgtasks/copy_s3_object.py:60-70` rewrites every
//! `<image-component src="…">`'s `src` and writes `str(soup)` back to
//! `description_html` (`update_description`, `:73-76`). The bytes are compared
//! by `difftest task` and the corpus in `tests/golden/bs4_html.json`
//! (`tools/inventory/bs4_corpus.py`) pins them.
//!
//! The port covers what bs4 4.12.3 does with `"html.parser"`:
//!
//! * the CPython `html.parser` tokenizer (`Lib/html/parser.py`, 3.12), with
//!   `convert_charrefs=False`: tag-name/attribute regexes, bogus comments,
//!   declarations, PIs, CDATA, RCDATA (`title`/`textarea`) and RAWTEXT
//!   (`script`, `style`, …) modes, and the close() handling of incomplete
//!   constructs;
//! * `HTMLParserTreeBuilder`'s tree: end tags pop to the most recent matching
//!   open tag and are otherwise ignored, void tags are closed when they get no
//!   children (and the duplicate `</br>` is swallowed once), whitespace-only
//!   data outside `pre`/`textarea` collapses to one space or one newline;
//! * the `"minimal"` formatter (`bs4/dammit.py:257-275`): text escapes
//!   `& < >`, attributes are sorted by name, `class`-like attributes are
//!   whitespace lists, values are escaped then quoted (`"` unless the value
//!   contains `"` but no `'`), void tags render `<br/>`, comments
//!   `<!--…-->`, the doctype `<!DOCTYPE …>\n` (a real newline is part of its
//!   suffix), and a `<meta charset>` is rewritten to `utf-8`.
//!
//! Not ported: the `XMLParsedAsHTMLWarning` (a `warnings.warn` call), line
//! numbers, and the `parse_only`/`SoupStrainer` machinery, none of which a
//! `str(soup)` call can observe.

use std::collections::HashMap;

use crate::bs4_entities;

/// `HTMLTreeBuilder.empty_element_tags` (`bs4/builder/__init__.py:389-395`).
const EMPTY_TAGS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "keygen", "link", "menuitem",
    "meta", "param", "source", "track", "wbr", "basefont", "bgsound", "command", "frame", "image",
    "isindex", "nextid", "spacer",
];

/// `HTMLParser.CDATA_CONTENT_ELEMENTS` plus `plaintext` (RAWTEXT mode).
const CDATA_ELEMENTS: &[&str] = &["script", "style", "xmp", "iframe", "noembed", "noframes"];

/// `HTMLParser.RCDATA_CONTENT_ELEMENTS`.
const RCDATA_ELEMENTS: &[&str] = &["textarea", "title"];

/// `HTMLTreeBuilder.DEFAULT_PRESERVE_WHITESPACE_TAGS`.
const PRESERVE_WHITESPACE: &[&str] = &["pre", "textarea"];

/// `HTMLTreeBuilder.DEFAULT_CDATA_LIST_ATTRIBUTES` (`bs4/builder/__init__.py:434`).
fn is_cdata_list_attribute(tag: &str, attr: &str) -> bool {
    matches!(attr, "class" | "accesskey" | "dropzone")
        || matches!(
            (tag, attr),
            ("a", "rel" | "rev")
                | ("link", "rel" | "rev")
                | ("td" | "th", "headers")
                | ("form", "accept-charset")
                | ("object", "archive")
                | ("area", "rel")
                | ("icon", "sizes")
                | ("iframe", "sandbox")
                | ("output", "for")
        )
}

/// `bs4.__init__.ASCII_SPACES`.
fn is_ascii_space(ch: char) -> bool {
    matches!(ch, '\x20' | '\x0a' | '\x09' | '\x0c' | '\x0d')
}

fn is_ws(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\x0c' | b'\r')
}

fn is_ascii_alpha(byte: u8) -> bool {
    byte.is_ascii_alphabetic()
}

/// `[^\t\n\r\f />]` — the end of a tag name.
fn is_tag_name_stop(byte: u8) -> bool {
    is_ws(byte) || byte == b'/' || byte == b'>'
}

/// The first character of an attribute name: `[^\t\n\r\f />]`.
fn is_attr_name_first(byte: u8) -> bool {
    !is_ws(byte) && byte != b'/' && byte != b'>'
}

/// The rest of an attribute name: `[^\t\n\r\f /=>]`.
fn is_attr_name_stop(byte: u8) -> bool {
    !is_ws(byte) && !matches!(byte, b'/' | b'=' | b'>')
}

/// The `(?<=['"\t\n\r\f /])` lookbehind shared by both attribute regexes.
fn attr_lookbehind(prev: Option<u8>) -> bool {
    matches!(
        prev,
        Some(b'\'' | b'"' | b' ' | b'\t' | b'\n' | b'\x0c' | b'\r' | b'/')
    )
}

/// A parsed tag attribute, after `BeautifulSoupHTMLParser.handle_starttag`.
#[derive(Debug, Clone)]
struct Attr {
    name: String,
    /// `Some("")` for `a=""` or a bare `a`; never `None` (the builder maps
    /// `None` to `""`).
    value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Tag,
    Text,
    Comment,
    Doctype,
    CData,
    Declaration,
    ProcessingInstruction,
}

#[derive(Debug)]
struct Node {
    kind: Kind,
    name: String,
    attrs: Vec<Attr>,
    children: Vec<usize>,
    text: String,
}

impl Node {
    fn new(kind: Kind) -> Self {
        Self {
            kind,
            name: String::new(),
            attrs: Vec::new(),
            children: Vec::new(),
            text: String::new(),
        }
    }
}

/// A parsed document that can be mutated and serialised.
///
/// `Soup::parse(html).to_string()` is `str(BeautifulSoup(html, "html.parser"))`.
#[derive(Debug)]
pub struct Soup {
    nodes: Vec<Node>,
    root: usize,
    /// The document's top-level nodes, in order.
    roots: Vec<usize>,
}

impl Soup {
    /// Parse one document. bs4 feeds the markup and then calls `close()`, so
    /// an incomplete construct at EOF takes the tokenizer's close branch.
    #[must_use]
    pub fn parse(html: &str) -> Self {
        let mut parser = Parser::new(html);
        parser.goahead(false);
        if parser.pos < parser.raw.len() {
            // `HTMLParser.close()` re-runs `goahead(1)` over the leftover.
            parser.raw = parser.raw[parser.pos..].to_string();
            parser.pos = 0;
            parser.goahead(true);
        }
        // `BeautifulSoup._feed` closes out unfinished strings (`:480`) and
        // then pops the open tags (no nodes are created by the pops).
        parser.end_data(None);
        let roots = parser.nodes[parser.root].children.clone();
        Self {
            nodes: parser.nodes,
            root: parser.root,
            roots,
        }
    }

    /// `soup.find_all(tag)`'s `src` attributes, in document order.
    ///
    /// `tag.get("src")` returns the attribute value; `extract_asset_ids`
    /// (`copy_s3_object.py:41-49`) reads it only when it is truthy.
    #[must_use]
    pub fn src_values(&self, tag: &str) -> Vec<String> {
        let mut out = Vec::new();
        self.collect_src_values(self.root, tag, &mut out);
        out
    }

    fn collect_src_values(&self, id: usize, tag: &str, out: &mut Vec<String>) {
        for &child in &self.nodes[id].children {
            let node = &self.nodes[child];
            if node.kind == Kind::Tag {
                if node.name == tag
                    && let Some(attr) = node.attrs.iter().find(|attr| attr.name == "src")
                    && !attr.value.is_empty()
                {
                    out.push(attr.value.clone());
                }
                self.collect_src_values(child, tag, out);
            }
        }
    }

    /// `replace_asset_ids`: for every `<tag>` whose `src` equals `old`, set it
    /// to `new`. bs4 keeps the attribute's position and only swaps the value.
    pub fn replace_src(&mut self, tag: &str, old: &str, new: &str) {
        for node in &mut self.nodes {
            if node.kind == Kind::Tag && node.name == tag {
                for attr in &mut node.attrs {
                    if attr.name == "src" && attr.value == old {
                        attr.value = new.to_string();
                    }
                }
            }
        }
    }

    /// `str(soup)`.
    #[must_use]
    pub fn serialize(&self) -> String {
        let mut out = String::new();
        for &child in &self.roots {
            self.write(child, false, &mut out);
        }
        out
    }

    fn write(&self, id: usize, raw_text: bool, out: &mut String) {
        let node = &self.nodes[id];
        match node.kind {
            Kind::Text => {
                if raw_text {
                    out.push_str(&node.text);
                } else {
                    escape_text(&node.text, out);
                }
            }
            Kind::Comment => {
                out.push_str("<!--");
                out.push_str(&node.text);
                out.push_str("-->");
            }
            Kind::Doctype => {
                out.push_str("<!DOCTYPE ");
                out.push_str(&node.text);
                out.push_str(">\n");
            }
            Kind::CData => {
                out.push_str("<![CDATA[");
                out.push_str(&node.text);
                out.push_str("]]>");
            }
            Kind::Declaration => {
                out.push_str("<?");
                out.push_str(&node.text);
                out.push_str("?>");
            }
            Kind::ProcessingInstruction => {
                out.push_str("<?");
                out.push_str(&node.text);
                out.push('>');
            }
            Kind::Tag => {
                let empty = node.children.is_empty() && EMPTY_TAGS.contains(&node.name.as_str());
                out.push('<');
                out.push_str(&node.name);
                write_attributes(node, out);
                if empty {
                    out.push_str("/>");
                    return;
                }
                out.push('>');
                let raw = node.name == "script" || node.name == "style";
                for &child in &node.children {
                    self.write(child, raw, out);
                }
                out.push_str("</");
                out.push_str(&node.name);
                out.push('>');
            }
        }
    }
}

/// `str(BeautifulSoup(html, "html.parser"))`.
#[must_use]
pub fn serialize(html: &str) -> String {
    Soup::parse(html).serialize()
}

/// `plane.utils.html_processor.strip_tags` (`plane/utils/html_processor.py:9-31`):
/// a `HTMLParser` with `convert_charrefs=True` that keeps only `handle_data`.
///
/// The `strip_tags` output is what `Page.save`/`Issue.save` write to
/// `description_stripped`. Unlike bs4 it does not collapse whitespace, skips
/// comments and declarations, and decodes character references in the data
/// (except inside `script`/`style`/…, whose `_escapable` is false).
#[must_use]
pub fn strip_tags(html: &str) -> String {
    let mut parser = Parser::new(html);
    parser.convert_charrefs = true;
    parser.sink = Some(String::new());
    // `strip_tags` calls `feed()` only (`html_processor.py:28-31`): it never
    // calls `HTMLParser.close()`, so an incomplete construct left at EOF (an
    // unterminated tag, comment or character reference) is dropped.
    parser.goahead(false);
    parser.sink.unwrap_or_default()
}

fn escape_text(text: &str, out: &mut String) {
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            other => out.push(other),
        }
    }
}

/// `EntitySubstitution.substitute_xml` (`bs4/dammit.py:257-275`).
fn escape_xml(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    escape_text(text, &mut out);
    out
}

/// `Formatter.quoted_attribute_value` (`bs4/formatter.py:220-255`).
fn quoted_attribute_value(value: &str) -> String {
    if value.contains('"') {
        if value.contains('\'') {
            return format!("\"{}\"", value.replace('"', "&quot;"));
        }
        return format!("'{value}'");
    }
    format!("\"{value}\"")
}

fn write_attributes(node: &Node, out: &mut String) {
    let mut attrs: Vec<&Attr> = node.attrs.iter().collect();
    // `Formatter.attributes` sorts by key (`bs4/formatter.py:141-146`).
    attrs.sort_by(|left, right| left.name.cmp(&right.name));

    // `HTMLTreeBuilder.set_up_substitutions` (`bs4/builder/__init__.py:458-490`).
    let meta_http_equiv = if node.name == "meta" {
        node.attrs
            .iter()
            .find(|attr| attr.name == "http-equiv")
            .map(|attr| attr.value.to_ascii_lowercase())
    } else {
        None
    };
    let meta_has_charset =
        node.name == "meta" && node.attrs.iter().any(|attr| attr.name == "charset");

    for attr in attrs {
        out.push(' ');
        out.push_str(&attr.name);
        out.push('=');
        let value = if node.name == "meta" && attr.name == "charset" && meta_has_charset {
            // `CharsetMetaAttributeValue.encode("utf-8")`.
            "utf-8".to_string()
        } else if node.name == "meta"
            && attr.name == "content"
            && meta_http_equiv.as_deref() == Some("content-type")
        {
            substitute_meta_charset(&attr.value)
        } else if is_cdata_list_attribute(&node.name, &attr.name) {
            // The builder turned the value into `\S+` words; output joins them.
            attr.value.split_whitespace().collect::<Vec<_>>().join(" ")
        } else {
            attr.value.clone()
        };
        out.push_str(&quoted_attribute_value(&escape_xml(&value)));
    }
}

/// `ContentMetaAttributeValue.encode` (`bs4/element.py:136-141`): replace the
/// value after a `charset=` with the output encoding.
fn substitute_meta_charset(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = String::new();
    let mut index = 0usize;
    let mut copied = 0usize;
    while index < bytes.len() {
        let at_start_or_semicolon = index == 0 || bytes[index - 1] == b';';
        if at_start_or_semicolon {
            let mut cursor = index;
            while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
                cursor += 1;
            }
            if value[cursor..].starts_with("charset=") {
                out.push_str(&value[copied..cursor + "charset=".len()]);
                let mut end = cursor + "charset=".len();
                while end < bytes.len() && bytes[end] != b';' {
                    end += 1;
                }
                out.push_str("utf-8");
                copied = end;
                index = end;
                continue;
            }
        }
        index += 1;
    }
    out.push_str(&value[copied..]);
    out
}

/// `rawdata[i:i+9].lower() == "<!doctype"` without slicing a non-boundary.
fn is_doctype(rest: &str) -> bool {
    rest.as_bytes()
        .get(..9)
        .is_some_and(|bytes| bytes.eq_ignore_ascii_case(b"<!doctype"))
}

/// The CPython `html.parser` tokenizer and bs4's `HTMLParserTreeBuilder`.
struct Parser {
    raw: String,
    pos: usize,
    nodes: Vec<Node>,
    root: usize,
    stack: Vec<usize>,
    open_count: HashMap<String, usize>,
    preserve: Vec<usize>,
    already_closed: Vec<String>,
    pending: String,
    pending_started: bool,
    cdata: Option<(String, bool)>,
    /// `convert_charrefs` (`html.parser`'s other mode, used by
    /// `plane.utils.html_processor.strip_tags`): data chunks are
    /// `html.unescape`d and `&` is not a token delimiter.
    convert_charrefs: bool,
    /// Collect only the text (`MLStripper`), never build a serialisable tree.
    sink: Option<String>,
}

impl Parser {
    fn new(html: &str) -> Self {
        let mut parser = Self {
            raw: html.to_string(),
            pos: 0,
            nodes: Vec::new(),
            root: 0,
            stack: Vec::new(),
            open_count: HashMap::new(),
            preserve: Vec::new(),
            already_closed: Vec::new(),
            pending: String::new(),
            pending_started: false,
            cdata: None,
            convert_charrefs: false,
            sink: None,
        };
        let mut root = Node::new(Kind::Tag);
        root.name = "[document]".to_string();
        parser.nodes.push(root);
        parser.stack.push(0);
        parser
    }

    fn current(&self) -> usize {
        *self.stack.last().expect("the root is always open")
    }

    fn new_node(&mut self, kind: Kind) -> usize {
        self.nodes.push(Node::new(kind));
        self.nodes.len() - 1
    }

    fn append(&mut self, id: usize) {
        let parent = self.current();
        self.nodes[parent].children.push(id);
    }

    // ---- tree building (bs4/__init__.py:558-770) -------------------------

    fn push_tag(&mut self, id: usize) {
        self.append(id);
        self.stack.push(id);
        let name = self.nodes[id].name.clone();
        if name != "[document]" {
            *self.open_count.entry(name.clone()).or_insert(0) += 1;
        }
        if PRESERVE_WHITESPACE.contains(&name.as_str()) {
            self.preserve.push(id);
        }
    }

    fn pop_tag(&mut self) {
        let id = self.stack.pop().expect("pop below the root");
        let name = self.nodes[id].name.clone();
        if let Some(count) = self.open_count.get_mut(&name) {
            *count = count.saturating_sub(1);
        }
        if self.preserve.last() == Some(&id) {
            self.preserve.pop();
        }
    }

    fn pop_to_tag(&mut self, name: &str) {
        if name == "[document]" || self.open_count.get(name).copied().unwrap_or(0) == 0 {
            return;
        }
        while self.stack.len() > 1 {
            let top = self.current();
            let matches = self.nodes[top].name == name;
            self.pop_tag();
            if matches {
                break;
            }
        }
    }

    fn end_data(&mut self, kind: Option<Kind>) {
        if self.sink.is_some() || !self.pending_started {
            return;
        }
        let mut text = std::mem::take(&mut self.pending);
        self.pending_started = false;
        if self.preserve.is_empty() && text.chars().all(is_ascii_space) {
            text = if text.contains('\n') {
                "\n".to_string()
            } else {
                " ".to_string()
            };
        }
        let id = self.new_node(kind.unwrap_or(Kind::Text));
        self.nodes[id].text = text;
        self.append(id);
    }

    fn handle_data(&mut self, data: &str) {
        if let Some(sink) = &mut self.sink {
            sink.push_str(data);
            return;
        }
        self.pending.push_str(data);
        self.pending_started = true;
    }

    fn handle_starttag(&mut self, name: &str, attrs: Vec<Attr>, handle_empty_element: bool) {
        self.end_data(None);
        let id = self.new_node(Kind::Tag);
        self.nodes[id].name = name.to_string();
        self.nodes[id].attrs = attrs;
        self.push_tag(id);
        let empty = self.nodes[id].children.is_empty() && EMPTY_TAGS.contains(&name);
        if empty && handle_empty_element {
            self.handle_endtag(name, false);
            self.already_closed.push(name.to_string());
        }
    }

    fn handle_endtag(&mut self, name: &str, check_already_closed: bool) {
        if check_already_closed
            && let Some(index) = self.already_closed.iter().position(|item| item == name)
        {
            self.already_closed.remove(index);
            return;
        }
        self.end_data(None);
        self.pop_to_tag(name);
    }

    // ---- tokenizer (Lib/html/parser.py:195-393) --------------------------

    fn goahead(&mut self, end: bool) {
        let n = self.raw.len();
        let mut i = self.pos;
        while i < n {
            let j = if let Some((name, escapable)) = self.cdata.clone() {
                self.next_cdata_marker(i, &name, escapable)
            } else if self.convert_charrefs {
                match self.raw[i..].find('<') {
                    Some(offset) => i + offset,
                    None => {
                        // `Lib/html/parser.py:207-213`: a trailing `&` may be
                        // the start of a reference split across feeds, so the
                        // chunk is held back rather than handed to
                        // `handle_data`. `strip_tags` never calls `close()`,
                        // so the held-back tail is dropped.
                        let start = i.max(n.saturating_sub(34));
                        if let Some(amp) = self.raw.rfind('&').filter(|amp| *amp >= start)
                            && !self.raw[amp..]
                                .chars()
                                .any(|ch| matches!(ch, '\t' | '\n' | '\r' | '\x0c' | ' ' | ';'))
                        {
                            break;
                        }
                        n
                    }
                }
            } else {
                match self.raw[i..].find(['&', '<']) {
                    Some(offset) => i + offset,
                    None => n,
                }
            };
            if i < j {
                let chunk = self.raw[i..j].to_string();
                let chunk = if self.convert_charrefs
                    && self.cdata.as_ref().is_none_or(|(_, escapable)| *escapable)
                {
                    self.unescape(&chunk)
                } else {
                    chunk
                };
                self.handle_data(&chunk);
            }
            i = j;
            if i == n {
                break;
            }
            if self.raw.as_bytes()[i] == b'<' {
                let k = if is_ascii_alpha(self.byte(i + 1)) {
                    self.parse_starttag(i)
                } else if self.raw[i..].starts_with("</") {
                    self.parse_endtag(i)
                } else if self.raw[i..].starts_with("<!--") {
                    self.parse_comment(i)
                } else if self.raw[i..].starts_with("<?") {
                    self.parse_pi(i)
                } else if self.raw[i..].starts_with("<!") {
                    self.parse_html_declaration(i)
                } else if i + 1 < n || end {
                    self.handle_data("<");
                    Some(i + 1)
                } else {
                    break;
                };
                let k = match k {
                    Some(k) => k,
                    None => {
                        if !end {
                            break;
                        }
                        if is_ascii_alpha(self.byte(i + 1)) {
                            // An incomplete start tag is dropped.
                        } else if self.raw[i..].starts_with("</") {
                            if i + 2 == n {
                                self.handle_data("</");
                            } else if is_ascii_alpha(self.byte(i + 2)) {
                                // An incomplete end tag is dropped.
                            } else {
                                // A bogus comment: `</` followed by anything else.
                                let data = self.raw[i + 2..].to_string();
                                self.comment(&data);
                            }
                        } else if self.raw[i..].starts_with("<!--") {
                            let mut stop = n;
                            for suffix in ["--!", "--", "-"] {
                                if self.raw[i..].ends_with(suffix) && stop >= i + 4 + suffix.len() {
                                    stop -= suffix.len();
                                    break;
                                }
                            }
                            let data = self.raw[i + 4..stop.max(i + 4)].to_string();
                            self.comment(&data);
                        } else if self.raw[i..].starts_with("<![CDATA[") {
                            let data = self.raw[i + 3..].to_string();
                            self.unknown_decl(&data);
                        } else if is_doctype(&self.raw[i..]) {
                            let data = self.raw[i + 2..].to_string();
                            self.declaration(&data);
                        } else if self.raw[i..].starts_with("<!") {
                            let data = self.raw[i + 2..].to_string();
                            self.comment(&data);
                        } else {
                            let data = self.raw[i + 2..].to_string();
                            self.processing_instruction(&data);
                        }
                        n
                    }
                };
                i = k;
                continue;
            }
            if self.raw[i..].starts_with("&#") {
                if let Some((name, next)) = self.charref(i) {
                    self.char_reference(&name);
                    i = next;
                    continue;
                }
                if self.raw[i..].contains(';') {
                    self.handle_data("&#");
                    i += 2;
                    continue;
                }
                break;
            }
            // `&` at i.
            if let Some((name, next)) = self.entityref(i) {
                self.entity_reference(&name);
                i = next;
                continue;
            }
            if self.raw[i..].len() <= 2 && self.raw[i..].starts_with("&") {
                if end && self.raw[i..].chars().count() == 2 {
                    i += 1;
                }
                break;
            }
            if i + 1 < n {
                self.handle_data("&");
                i += 1;
                continue;
            }
            break;
        }
        // `if end and i < n` (`Lib/html/parser.py:324-330`): with
        // `convert_charrefs=False` the leftover is data verbatim.
        if end && i < n {
            let rest = self.raw[i..].to_string();
            self.handle_data(&rest);
            i = n;
        }
        self.pos = i;
    }

    fn byte(&self, index: usize) -> u8 {
        self.raw.as_bytes().get(index).copied().unwrap_or(0)
    }

    /// `interesting.search` in RAWTEXT/RCDATA mode.
    fn next_cdata_marker(&self, from: usize, name: &str, escapable: bool) -> usize {
        if name == "plaintext" {
            return self.raw.len();
        }
        let rest = &self.raw[from..];
        let lower = rest.to_ascii_lowercase();
        let marker = format!("</{name}");
        let mut best: Option<usize> = None;
        let mut start = 0usize;
        while let Some(offset) = lower[start..].find(&marker) {
            let position = start + offset;
            let after = position + marker.len();
            let terminates = after >= rest.len()
                || matches!(
                    self.byte(from + after),
                    b' ' | b'\t' | b'\n' | b'\x0c' | b'\r' | b'/' | b'>'
                );
            if terminates {
                best = Some(position);
                break;
            }
            start = position + 1;
        }
        if escapable
            && !self.convert_charrefs
            && let Some(amp) = rest.find('&')
            && best.is_none_or(|marker| amp < marker)
        {
            best = Some(amp);
        }
        best.map_or(self.raw.len(), |position| from + position)
    }

    /// `parse_starttag`; `None` is Python's `-1`.
    fn parse_starttag(&mut self, i: usize) -> Option<usize> {
        let endpos = self.locate_tag_end(i + 1)?;
        let mut k = i + 1;
        let name_start = k;
        if !is_ascii_alpha(self.byte(k)) {
            return None;
        }
        k += 1;
        while k < endpos && !is_tag_name_stop(self.byte(k)) {
            k += 1;
        }
        let name = self.raw[name_start..k].to_ascii_lowercase();
        k = self.skip_tag_whitespace(k);

        let mut attrs = Vec::new();
        while k < endpos {
            let Some((attr_name, value, next)) = self.attrfind(k) else {
                break;
            };
            let value = value.unwrap_or_default();
            let value = if value.is_empty() {
                value
            } else {
                self.unescape(&value)
            };
            // `on_duplicate_attribute=REPLACE`: the last value wins, the
            // key keeps its first position (`bs4/builder/_htmlparser.py:115-133`).
            let name = attr_name.to_ascii_lowercase();
            if let Some(existing) = attrs.iter_mut().find(|attr: &&mut Attr| attr.name == name) {
                existing.value = value;
            } else {
                attrs.push(Attr { name, value });
            }
            k = next;
        }

        let tail = self.raw[k.min(endpos)..endpos].trim();
        if tail != ">" && tail != "/>" {
            let data = self.raw[i..endpos].to_string();
            self.handle_data(&data);
            return Some(endpos);
        }
        if tail == "/>" {
            self.handle_starttag(&name, attrs, false);
            self.handle_endtag(&name, true);
        } else {
            self.handle_starttag(&name, attrs, true);
            if CDATA_ELEMENTS.contains(&name.as_str()) || name == "plaintext" {
                self.cdata = Some((name, false));
            } else if RCDATA_ELEMENTS.contains(&name.as_str()) {
                self.cdata = Some((name, true));
            }
        }
        Some(endpos)
    }

    /// `locatetagend.match(rawdata, j)`; `None` when it does not end on `>`.
    fn locate_tag_end(&self, j: usize) -> Option<usize> {
        let n = self.raw.len();
        if j >= n || !is_ascii_alpha(self.byte(j)) {
            return None;
        }
        let mut cursor = j + 1;
        while cursor < n && !is_tag_name_stop(self.byte(cursor)) {
            cursor += 1;
        }
        while cursor < n && (is_ws(self.byte(cursor)) || self.byte(cursor) == b'/') {
            cursor += 1;
        }
        loop {
            if cursor >= n {
                break;
            }
            if !attr_lookbehind(cursor.checked_sub(1).map(|index| self.byte(index)))
                || !is_attr_name_first(self.byte(cursor))
            {
                break;
            }
            let after_name = self.skip_attr_name(cursor);
            let mut next = self.skip_ws(after_name);
            if next < n && self.byte(next) == b'=' {
                next += 1;
                next = self.skip_ws(next);
                if next < n && matches!(self.byte(next), b'\'' | b'"') {
                    let quote = self.byte(next);
                    let Some(close) = self.find_byte(next + 1, quote) else {
                        cursor = after_name;
                        break;
                    };
                    next = close + 1;
                } else {
                    while next < n && !is_ws(self.byte(next)) && self.byte(next) != b'>' {
                        next += 1;
                    }
                }
                cursor = next;
            } else {
                cursor = after_name;
            }
            while cursor < n && (is_ws(self.byte(cursor)) || self.byte(cursor) == b'/') {
                cursor += 1;
            }
        }
        if cursor < n && self.byte(cursor) == b'>' {
            Some(cursor + 1)
        } else {
            None
        }
    }

    /// `attrfind_tolerant.match(rawdata, k)` → `(name, value, end)`.
    fn attrfind(&self, k: usize) -> Option<(&str, Option<String>, usize)> {
        let n = self.raw.len();
        if k >= n
            || !attr_lookbehind(k.checked_sub(1).map(|index| self.byte(index)))
            || !is_attr_name_first(self.byte(k))
        {
            return None;
        }
        let name_start = k;
        let after_name = self.skip_attr_name(k);
        let name = &self.raw[name_start..after_name];
        let mut cursor = after_name;
        let mut value = None;
        let mut next = self.skip_ws(after_name);
        if next < n && self.byte(next) == b'=' {
            next += 1;
            next = self.skip_ws(next);
            if next < n && matches!(self.byte(next), b'\'' | b'"') {
                let quote = self.byte(next);
                if let Some(close) = self.find_byte(next + 1, quote) {
                    value = Some(self.raw[next + 1..close].to_string());
                    cursor = close + 1;
                }
            } else {
                let start = next;
                while next < n && !is_ws(self.byte(next)) && self.byte(next) != b'>' {
                    next += 1;
                }
                value = Some(self.raw[start..next].to_string());
                cursor = next;
            }
        }
        while cursor < n
            && (is_ws(self.byte(cursor))
                || (self.byte(cursor) == b'/' && self.byte(cursor + 1) != b'>'))
        {
            cursor += 1;
        }
        Some((name, value, cursor))
    }

    fn skip_attr_name(&self, from: usize) -> usize {
        let n = self.raw.len();
        let mut cursor = from + 1;
        while cursor < n && is_attr_name_stop(self.byte(cursor)) {
            cursor += 1;
        }
        cursor
    }

    fn skip_ws(&self, from: usize) -> usize {
        let n = self.raw.len();
        let mut cursor = from;
        while cursor < n && is_ws(self.byte(cursor)) {
            cursor += 1;
        }
        cursor
    }

    /// `tagfind_tolerant`'s trailing `(?:[\t\n\r\f ]|/(?!>))*`.
    fn skip_tag_whitespace(&self, from: usize) -> usize {
        let n = self.raw.len();
        let mut cursor = from;
        while cursor < n {
            let byte = self.byte(cursor);
            if is_ws(byte) || (byte == b'/' && self.byte(cursor + 1) != b'>') {
                cursor += 1;
            } else {
                break;
            }
        }
        cursor
    }

    fn find_byte(&self, from: usize, needle: u8) -> Option<usize> {
        self.raw.as_bytes()[from..]
            .iter()
            .position(|byte| *byte == needle)
            .map(|offset| from + offset)
    }

    /// `parse_endtag`; `None` is Python's `-1`.
    fn parse_endtag(&mut self, i: usize) -> Option<usize> {
        self.find_byte(i + 2, b'>')?;
        if !is_ascii_alpha(self.byte(i + 2)) {
            if self.byte(i + 2) == b'>' {
                return Some(i + 3);
            }
            return self.parse_bogus_comment(i);
        }
        let end = self.locate_tag_end(i + 2)?;
        let mut stop = i + 3;
        while stop < end && !is_tag_name_stop(self.byte(stop)) {
            stop += 1;
        }
        let name = self.raw[i + 2..stop].to_ascii_lowercase();
        self.handle_endtag(&name, true);
        self.cdata = None;
        Some(end)
    }

    /// `parse_comment`; `None` is Python's `-1`.
    fn parse_comment(&mut self, i: usize) -> Option<usize> {
        let n = self.raw.len();
        let mut close = None;
        let mut cursor = i + 4;
        while cursor < n {
            if self.raw[cursor..].starts_with("-->") {
                close = Some((cursor, cursor + 3));
                break;
            }
            if self.raw[cursor..].starts_with("--!>") {
                close = Some((cursor, cursor + 4));
                break;
            }
            cursor += 1;
        }
        let close = close.or_else(|| {
            if self.raw[i + 4..].starts_with("->") {
                Some((i + 4, i + 6))
            } else if self.raw[i + 4..].starts_with('>') {
                Some((i + 4, i + 5))
            } else {
                None
            }
        });
        let (stop, next) = close?;
        let data = self.raw[i + 4..stop].to_string();
        self.comment(&data);
        Some(next)
    }

    fn parse_bogus_comment(&mut self, i: usize) -> Option<usize> {
        let stop = self.find_byte(i + 2, b'>')?;
        let data = self.raw[i + 2..stop].to_string();
        self.comment(&data);
        Some(stop + 1)
    }

    fn parse_pi(&mut self, i: usize) -> Option<usize> {
        let stop = self.find_byte(i + 2, b'>')?;
        let data = self.raw[i + 2..stop].to_string();
        self.processing_instruction(&data);
        Some(stop + 1)
    }

    fn parse_html_declaration(&mut self, i: usize) -> Option<usize> {
        if self.raw[i..].starts_with("<!--") {
            return self.parse_comment(i);
        }
        if self.raw[i..].starts_with("<![CDATA[") {
            let stop = self.raw[i + 9..].find("]]>")? + i + 9;
            let data = self.raw[i + 3..stop].to_string();
            self.unknown_decl(&data);
            return Some(stop + 3);
        }
        if is_doctype(&self.raw[i..]) {
            let stop = self.find_byte(i + 9, b'>')?;
            let data = self.raw[i + 2..stop].to_string();
            self.declaration(&data);
            return Some(stop + 1);
        }
        if self.raw[i..].starts_with("<![") {
            let stop = self.find_byte(i + 3, b'>')?;
            if self.byte(stop - 1) == b']' {
                let data = self.raw[i + 3..stop - 1].to_string();
                self.unknown_decl(&data);
            } else {
                let data = self.raw[i + 2..stop].to_string();
                self.comment(&data);
            }
            return Some(stop + 1);
        }
        self.parse_bogus_comment(i)
    }

    /// `charref` at `i` → `(name, next)`.
    fn charref(&self, i: usize) -> Option<(String, usize)> {
        let n = self.raw.len();
        let mut cursor = i + 2;
        let hex = self.byte(cursor) == b'x' || self.byte(cursor) == b'X';
        if hex {
            cursor += 1;
            let start = cursor;
            while cursor < n && self.byte(cursor).is_ascii_hexdigit() {
                cursor += 1;
            }
            if cursor == start {
                return None;
            }
        } else {
            let start = cursor;
            while cursor < n && self.byte(cursor).is_ascii_digit() {
                cursor += 1;
            }
            if cursor == start {
                return None;
            }
        }
        if cursor >= n {
            return None;
        }
        if self.byte(cursor).is_ascii_hexdigit() {
            return None;
        }
        // `handle_charref` receives the name without `&#` but with the `x`.
        let name = self.raw[i + 2..cursor].to_string();
        // The delimiter is consumed only when it is `;`.
        let next = if self.byte(cursor) == b';' {
            cursor + 1
        } else {
            cursor
        };
        Some((name, next))
    }

    /// `entityref` at `i` → `(name, next)`.
    fn entityref(&self, i: usize) -> Option<(String, usize)> {
        let n = self.raw.len();
        let mut cursor = i + 1;
        if cursor >= n || !self.byte(cursor).is_ascii_alphabetic() {
            return None;
        }
        cursor += 1;
        while cursor < n {
            let byte = self.byte(cursor);
            if byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'.' {
                cursor += 1;
            } else {
                break;
            }
        }
        if cursor >= n {
            return None;
        }
        let name = self.raw[i + 1..cursor].to_string();
        let next = if self.byte(cursor) == b';' {
            cursor + 1
        } else {
            cursor
        };
        Some((name, next))
    }

    /// Python's `html.unescape`, which bs4 applies to non-empty attribute
    /// values (`Lib/html/parser.py:531-538`). Numeric references take an
    /// optional semicolon; a named reference is looked up without it and an
    /// unknown name is left as written (`&foo;` keeps its semicolon).
    fn unescape(&self, value: &str) -> String {
        let mut out = String::with_capacity(value.len());
        let mut cursor = 0usize;
        while let Some(offset) = value[cursor..].find('&') {
            let amp = cursor + offset;
            out.push_str(&value[cursor..amp]);
            let tail = &value[amp + 1..];
            if let Some(after_hash) = tail.strip_prefix('#') {
                let (hex, digits) = match after_hash.strip_prefix(['x', 'X']) {
                    Some(hex) => (true, hex),
                    None => (false, after_hash),
                };
                let count = digits
                    .chars()
                    .take_while(|ch| {
                        if hex {
                            ch.is_ascii_hexdigit()
                        } else {
                            ch.is_ascii_digit()
                        }
                    })
                    .count();
                if count > 0 {
                    let radix = if hex { 16 } else { 10 };
                    if let Ok(code) = u32::from_str_radix(&digits[..count], radix) {
                        out.push_str(&html_unescape_number(code));
                        let mut next = amp + 2 + count + usize::from(hex);
                        if value[next..].starts_with(';') {
                            next += 1;
                        }
                        cursor = next;
                        continue;
                    }
                }
                out.push('&');
                cursor = amp + 1;
                continue;
            }
            let count = tail
                .chars()
                .take(32)
                .take_while(|ch| !matches!(ch, '\t' | '\n' | '\x0c' | ' ' | '<' | '&' | '#' | ';'))
                .count();
            let name = &tail[..count];
            let mut replaced = false;
            if !name.is_empty() {
                if let Some(replacement) = bs4_entities::lookup(name) {
                    out.push_str(replacement);
                    replaced = true;
                } else {
                    // The longest matching prefix (`Lib/html/__init__.py:110-115`),
                    // so `&notit;` is `¬it;`.
                    let chars: Vec<char> = name.chars().collect();
                    for cut in (2..chars.len()).rev() {
                        let prefix: String = chars[..cut].iter().collect();
                        if let Some(replacement) = bs4_entities::lookup(&prefix) {
                            out.push_str(replacement);
                            out.extend(&chars[cut..]);
                            replaced = true;
                            break;
                        }
                    }
                }
            }
            if replaced {
                let mut next = amp + 1 + count;
                if value[next..].starts_with(';') {
                    next += 1;
                }
                cursor = next;
                continue;
            }
            out.push('&');
            cursor = amp + 1;
        }
        out.push_str(&value[cursor..]);
        out
    }

    // ---- builder callbacks that create nodes -----------------------------

    fn comment(&mut self, data: &str) {
        if self.sink.is_some() {
            return;
        }
        self.end_data(None);
        self.handle_data(data);
        self.end_data(Some(Kind::Comment));
    }

    fn declaration(&mut self, data: &str) {
        if self.sink.is_some() {
            return;
        }
        self.end_data(None);
        let text: String = data.chars().skip(8).collect();
        self.handle_data(&text);
        self.end_data(Some(Kind::Doctype));
    }

    fn unknown_decl(&mut self, data: &str) {
        if self.sink.is_some() {
            return;
        }
        self.end_data(None);
        if data.len() >= 6 && data[..6].eq_ignore_ascii_case("CDATA[") {
            let text = data[6..].to_string();
            self.handle_data(&text);
            self.end_data(Some(Kind::CData));
        } else {
            self.handle_data(data);
            self.end_data(Some(Kind::Declaration));
        }
    }

    fn processing_instruction(&mut self, data: &str) {
        if self.sink.is_some() {
            return;
        }
        self.end_data(None);
        self.handle_data(data);
        self.end_data(Some(Kind::ProcessingInstruction));
    }

    fn char_reference(&mut self, name: &str) {
        let code = if let Some(hex) = name.strip_prefix(['x', 'X']) {
            u32::from_str_radix(hex, 16).ok()
        } else {
            name.parse::<u32>().ok()
        };
        let text = match code {
            Some(code) => code_point(code).to_string(),
            None => "\u{FFFD}".to_string(),
        };
        self.handle_data(&text);
    }

    fn entity_reference(&mut self, name: &str) {
        if let Some(replacement) = bs4_entities::lookup(name) {
            self.handle_data(replacement);
        } else {
            let text = format!("&{name}");
            self.handle_data(&text);
        }
    }
}

/// `html._replace_charref`'s numeric branch (`Lib/html/__init__.py:94-105`).
///
/// Used by `html.unescape` (attribute values and `strip_tags` data), which is
/// not the same mapping as bs4's `handle_charref`: the C1 range is Windows-1252
/// (controls kept as themselves), `0x00` is U+FFFD, and `_invalid_codepoints`
/// disappear from the output entirely.
fn html_unescape_number(code: u32) -> String {
    let c1 = match code {
        0x00 => Some('\u{FFFD}'),
        0x0D => Some('\r'),
        0x80 => Some('\u{20AC}'),
        0x81 => Some('\u{81}'),
        0x82 => Some('\u{201A}'),
        0x83 => Some('\u{0192}'),
        0x84 => Some('\u{201E}'),
        0x85 => Some('\u{2026}'),
        0x86 => Some('\u{2020}'),
        0x87 => Some('\u{2021}'),
        0x88 => Some('\u{02C6}'),
        0x89 => Some('\u{2030}'),
        0x8A => Some('\u{0160}'),
        0x8B => Some('\u{2039}'),
        0x8C => Some('\u{0152}'),
        0x8D => Some('\u{8D}'),
        0x8E => Some('\u{017D}'),
        0x8F => Some('\u{8F}'),
        0x90 => Some('\u{90}'),
        0x91 => Some('\u{2018}'),
        0x92 => Some('\u{2019}'),
        0x93 => Some('\u{201C}'),
        0x94 => Some('\u{201D}'),
        0x95 => Some('\u{2022}'),
        0x96 => Some('\u{2013}'),
        0x97 => Some('\u{2014}'),
        0x98 => Some('\u{02DC}'),
        0x99 => Some('\u{2122}'),
        0x9A => Some('\u{0161}'),
        0x9B => Some('\u{203A}'),
        0x9C => Some('\u{0153}'),
        0x9D => Some('\u{9D}'),
        0x9E => Some('\u{017E}'),
        0x9F => Some('\u{0178}'),
        _ => None,
    };
    if let Some(ch) = c1 {
        return ch.to_string();
    }
    if (0xD800..=0xDFFF).contains(&code) || code > 0x10FFFF {
        return "\u{FFFD}".to_string();
    }
    let dropped = matches!(
        code,
        0x1..=0x8
            | 0x0B
            | 0x0E..=0x1F
            | 0x7F..=0x9F
            | 0xFDD0..=0xFDEF
            | 0xFFFE
            | 0xFFFF
            | 0x1FFFE
            | 0x1FFFF
            | 0x2FFFE
            | 0x2FFFF
            | 0x3FFFE
            | 0x3FFFF
            | 0x4FFFE
            | 0x4FFFF
            | 0x5FFFE
            | 0x5FFFF
            | 0x6FFFE
            | 0x6FFFF
            | 0x7FFFE
            | 0x7FFFF
            | 0x8FFFE
            | 0x8FFFF
            | 0x9FFFE
            | 0x9FFFF
            | 0xAFFFE
            | 0xAFFFF
            | 0xBFFFE
            | 0xBFFFF
            | 0xCFFFE
            | 0xCFFFF
            | 0xDFFFE
            | 0xDFFFF
            | 0xEFFFE
            | 0xEFFFF
            | 0xFFFFE
            | 0xFFFFF
            | 0x10FFFE
            | 0x10FFFF
    );
    if dropped {
        return String::new();
    }
    char::from_u32(code).unwrap_or('\u{FFFD}').to_string()
}

/// `BeautifulSoupHTMLParser.handle_charref`'s windows-1252 quirk
/// (`bs4/builder/_htmlparser.py:180-215`) followed by `chr()`.
fn code_point(code: u32) -> char {
    if code < 256
        && let Some(ch) = windows_1252(code as u8)
    {
        return ch;
    }
    char::from_u32(code).unwrap_or('\u{FFFD}')
}

/// The bytes windows-1252 leaves undefined make Python's decode fail.
fn windows_1252(byte: u8) -> Option<char> {
    let special = match byte {
        0x80 => Some('€'),
        0x82 => Some('‚'),
        0x83 => Some('ƒ'),
        0x84 => Some('„'),
        0x85 => Some('…'),
        0x86 => Some('†'),
        0x87 => Some('‡'),
        0x88 => Some('ˆ'),
        0x89 => Some('‰'),
        0x8A => Some('Š'),
        0x8B => Some('‹'),
        0x8C => Some('Œ'),
        0x8E => Some('Ž'),
        0x91 => Some('‘'),
        0x92 => Some('’'),
        0x93 => Some('“'),
        0x94 => Some('”'),
        0x95 => Some('•'),
        0x96 => Some('–'),
        0x97 => Some('—'),
        0x98 => Some('˜'),
        0x99 => Some('™'),
        0x9A => Some('š'),
        0x9B => Some('›'),
        0x9C => Some('œ'),
        0x9E => Some('ž'),
        0x9F => Some('Ÿ'),
        0x81 | 0x8D | 0x8F | 0x90 | 0x9D => return None,
        _ => None,
    };
    if let Some(ch) = special {
        return Some(ch);
    }
    Some(char::from(byte))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attributes_are_sorted_and_quoted_like_bs4() {
        let soup = Soup::parse("<p title='he said \"hi\"' data-x=\"a'b\" empty=\"\">q</p>");
        assert_eq!(
            soup.serialize(),
            "<p data-x=\"a'b\" empty=\"\" title='he said \"hi\"'>q</p>"
        );
    }

    #[test]
    fn void_tags_render_self_closing_and_duplicate_end_tags_are_swallowed() {
        assert_eq!(
            serialize("<p>a<br></br>b<img src=\"u\"></img>c</p>"),
            "<p>a<br/>b<img src=\"u\"/>c</p>"
        );
    }

    #[test]
    fn a_doctype_suffix_ends_in_a_newline_and_a_meta_charset_is_rewritten() {
        assert_eq!(
            serialize("<meta charset=\"utf8\"><p>x</p>"),
            "<meta charset=\"utf-8\"/><p>x</p>"
        );
        assert_eq!(
            serialize("<!DOCTYPE html><p>x</p>"),
            "<!DOCTYPE html>\n<p>x</p>"
        );
    }

    #[test]
    fn image_component_src_is_rewritten_in_place() {
        let mut soup =
            Soup::parse("<p><image-component src=\"old\" width=\"1\"></image-component></p>");
        assert_eq!(soup.src_values("image-component"), vec!["old".to_string()]);
        soup.replace_src("image-component", "old", "new");
        assert_eq!(
            soup.serialize(),
            "<p><image-component src=\"new\" width=\"1\"></image-component></p>"
        );
    }

    #[test]
    fn whitespace_only_text_collapses_outside_pre() {
        assert_eq!(serialize("<p>\n   </p>"), "<p>\n</p>");
        assert_eq!(serialize("<pre>  x\n  y </pre>"), "<pre>  x\n  y </pre>");
    }
}
