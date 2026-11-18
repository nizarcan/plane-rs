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

    /// `htmlStartClose`: `(open tag, tag that implies its end)`
    /// (`HTMLparser.c`, `htmlStartClose`).
    const START_CLOSE: [(&str, &str); 249] = [
        ("a", "a"),
        ("a", "fieldset"),
        ("a", "table"),
        ("a", "td"),
        ("a", "th"),
        ("address", "dd"),
        ("address", "dl"),
        ("address", "dt"),
        ("address", "form"),
        ("address", "li"),
        ("address", "ul"),
        ("b", "center"),
        ("b", "p"),
        ("b", "td"),
        ("b", "th"),
        ("big", "p"),
        ("caption", "col"),
        ("caption", "colgroup"),
        ("caption", "tbody"),
        ("caption", "tfoot"),
        ("caption", "thead"),
        ("caption", "tr"),
        ("col", "col"),
        ("col", "colgroup"),
        ("col", "tbody"),
        ("col", "tfoot"),
        ("col", "thead"),
        ("col", "tr"),
        ("colgroup", "colgroup"),
        ("colgroup", "tbody"),
        ("colgroup", "tfoot"),
        ("colgroup", "thead"),
        ("colgroup", "tr"),
        ("dd", "dt"),
        ("dir", "dd"),
        ("dir", "dl"),
        ("dir", "dt"),
        ("dir", "form"),
        ("dir", "ul"),
        ("dl", "form"),
        ("dl", "li"),
        ("dt", "dd"),
        ("dt", "dl"),
        ("font", "center"),
        ("font", "td"),
        ("font", "th"),
        ("form", "form"),
        ("h1", "fieldset"),
        ("h1", "form"),
        ("h1", "li"),
        ("h1", "p"),
        ("h1", "table"),
        ("h2", "fieldset"),
        ("h2", "form"),
        ("h2", "li"),
        ("h2", "p"),
        ("h2", "table"),
        ("h3", "fieldset"),
        ("h3", "form"),
        ("h3", "li"),
        ("h3", "p"),
        ("h3", "table"),
        ("h4", "fieldset"),
        ("h4", "form"),
        ("h4", "li"),
        ("h4", "p"),
        ("h4", "table"),
        ("h5", "fieldset"),
        ("h5", "form"),
        ("h5", "li"),
        ("h5", "p"),
        ("h5", "table"),
        ("h6", "fieldset"),
        ("h6", "form"),
        ("h6", "li"),
        ("h6", "p"),
        ("h6", "table"),
        ("head", "a"),
        ("head", "abbr"),
        ("head", "acronym"),
        ("head", "address"),
        ("head", "b"),
        ("head", "bdo"),
        ("head", "big"),
        ("head", "blockquote"),
        ("head", "body"),
        ("head", "br"),
        ("head", "center"),
        ("head", "cite"),
        ("head", "code"),
        ("head", "dd"),
        ("head", "dfn"),
        ("head", "dir"),
        ("head", "div"),
        ("head", "dl"),
        ("head", "dt"),
        ("head", "em"),
        ("head", "fieldset"),
        ("head", "font"),
        ("head", "form"),
        ("head", "frameset"),
        ("head", "h1"),
        ("head", "h2"),
        ("head", "h3"),
        ("head", "h4"),
        ("head", "h5"),
        ("head", "h6"),
        ("head", "hr"),
        ("head", "i"),
        ("head", "iframe"),
        ("head", "img"),
        ("head", "kbd"),
        ("head", "li"),
        ("head", "listing"),
        ("head", "map"),
        ("head", "menu"),
        ("head", "ol"),
        ("head", "p"),
        ("head", "pre"),
        ("head", "q"),
        ("head", "s"),
        ("head", "samp"),
        ("head", "small"),
        ("head", "span"),
        ("head", "strike"),
        ("head", "strong"),
        ("head", "sub"),
        ("head", "sup"),
        ("head", "table"),
        ("head", "tt"),
        ("head", "u"),
        ("head", "ul"),
        ("head", "var"),
        ("head", "xmp"),
        ("hr", "form"),
        ("i", "center"),
        ("i", "p"),
        ("i", "td"),
        ("i", "th"),
        ("legend", "fieldset"),
        ("li", "li"),
        ("link", "body"),
        ("link", "frameset"),
        ("listing", "dd"),
        ("listing", "dl"),
        ("listing", "dt"),
        ("listing", "fieldset"),
        ("listing", "form"),
        ("listing", "li"),
        ("listing", "table"),
        ("listing", "ul"),
        ("menu", "dd"),
        ("menu", "dl"),
        ("menu", "dt"),
        ("menu", "form"),
        ("menu", "ul"),
        ("ol", "form"),
        ("option", "optgroup"),
        ("option", "option"),
        ("p", "address"),
        ("p", "blockquote"),
        ("p", "body"),
        ("p", "caption"),
        ("p", "center"),
        ("p", "col"),
        ("p", "colgroup"),
        ("p", "dd"),
        ("p", "dir"),
        ("p", "div"),
        ("p", "dl"),
        ("p", "dt"),
        ("p", "fieldset"),
        ("p", "form"),
        ("p", "frameset"),
        ("p", "h1"),
        ("p", "h2"),
        ("p", "h3"),
        ("p", "h4"),
        ("p", "h5"),
        ("p", "h6"),
        ("p", "head"),
        ("p", "hr"),
        ("p", "li"),
        ("p", "listing"),
        ("p", "menu"),
        ("p", "ol"),
        ("p", "p"),
        ("p", "pre"),
        ("p", "table"),
        ("p", "tbody"),
        ("p", "td"),
        ("p", "tfoot"),
        ("p", "th"),
        ("p", "title"),
        ("p", "tr"),
        ("p", "ul"),
        ("p", "xmp"),
        ("pre", "dd"),
        ("pre", "dl"),
        ("pre", "dt"),
        ("pre", "fieldset"),
        ("pre", "form"),
        ("pre", "li"),
        ("pre", "table"),
        ("pre", "ul"),
        ("s", "p"),
        ("script", "noscript"),
        ("small", "p"),
        ("span", "td"),
        ("span", "th"),
        ("strike", "p"),
        ("style", "body"),
        ("style", "frameset"),
        ("tbody", "tbody"),
        ("tbody", "tfoot"),
        ("td", "tbody"),
        ("td", "td"),
        ("td", "tfoot"),
        ("td", "th"),
        ("td", "tr"),
        ("tfoot", "tbody"),
        ("th", "tbody"),
        ("th", "td"),
        ("th", "tfoot"),
        ("th", "th"),
        ("th", "tr"),
        ("thead", "tbody"),
        ("thead", "tfoot"),
        ("title", "body"),
        ("title", "frameset"),
        ("tr", "tbody"),
        ("tr", "tfoot"),
        ("tr", "tr"),
        ("tt", "p"),
        ("u", "p"),
        ("u", "td"),
        ("u", "th"),
        ("ul", "address"),
        ("ul", "form"),
        ("ul", "menu"),
        ("ul", "pre"),
        ("xmp", "dd"),
        ("xmp", "dl"),
        ("xmp", "dt"),
        ("xmp", "fieldset"),
        ("xmp", "form"),
        ("xmp", "li"),
        ("xmp", "table"),
        ("xmp", "ul"),
    ];

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

    /// The WHATWG named character reference table, sorted by name: `(name,
    /// replacement, requires semicolon)`.
    ///
    /// This is libxml2's `html5ent.inc` (2 125 names) as exposed by CPython
    /// 3.12's `html.entities.html5`; the two are generated from the same
    /// source and agree entry for entry. The third field is libxml2's
    /// `ENT_F_SEMICOLON`: a name without it may also terminate on a
    /// non-alphanumeric character (`&amp` → `&`).
    static ENTITIES: [(&str, &str, bool); 2125] = [
        ("AElig", "\u{c6}", false),
        ("AMP", "\u{26}", false),
        ("Aacute", "\u{c1}", false),
        ("Abreve", "\u{102}", true),
        ("Acirc", "\u{c2}", false),
        ("Acy", "\u{410}", true),
        ("Afr", "\u{1d504}", true),
        ("Agrave", "\u{c0}", false),
        ("Alpha", "\u{391}", true),
        ("Amacr", "\u{100}", true),
        ("And", "\u{2a53}", true),
        ("Aogon", "\u{104}", true),
        ("Aopf", "\u{1d538}", true),
        ("ApplyFunction", "\u{2061}", true),
        ("Aring", "\u{c5}", false),
        ("Ascr", "\u{1d49c}", true),
        ("Assign", "\u{2254}", true),
        ("Atilde", "\u{c3}", false),
        ("Auml", "\u{c4}", false),
        ("Backslash", "\u{2216}", true),
        ("Barv", "\u{2ae7}", true),
        ("Barwed", "\u{2306}", true),
        ("Bcy", "\u{411}", true),
        ("Because", "\u{2235}", true),
        ("Bernoullis", "\u{212c}", true),
        ("Beta", "\u{392}", true),
        ("Bfr", "\u{1d505}", true),
        ("Bopf", "\u{1d539}", true),
        ("Breve", "\u{2d8}", true),
        ("Bscr", "\u{212c}", true),
        ("Bumpeq", "\u{224e}", true),
        ("CHcy", "\u{427}", true),
        ("COPY", "\u{a9}", false),
        ("Cacute", "\u{106}", true),
        ("Cap", "\u{22d2}", true),
        ("CapitalDifferentialD", "\u{2145}", true),
        ("Cayleys", "\u{212d}", true),
        ("Ccaron", "\u{10c}", true),
        ("Ccedil", "\u{c7}", false),
        ("Ccirc", "\u{108}", true),
        ("Cconint", "\u{2230}", true),
        ("Cdot", "\u{10a}", true),
        ("Cedilla", "\u{b8}", true),
        ("CenterDot", "\u{b7}", true),
        ("Cfr", "\u{212d}", true),
        ("Chi", "\u{3a7}", true),
        ("CircleDot", "\u{2299}", true),
        ("CircleMinus", "\u{2296}", true),
        ("CirclePlus", "\u{2295}", true),
        ("CircleTimes", "\u{2297}", true),
        ("ClockwiseContourIntegral", "\u{2232}", true),
        ("CloseCurlyDoubleQuote", "\u{201d}", true),
        ("CloseCurlyQuote", "\u{2019}", true),
        ("Colon", "\u{2237}", true),
        ("Colone", "\u{2a74}", true),
        ("Congruent", "\u{2261}", true),
        ("Conint", "\u{222f}", true),
        ("ContourIntegral", "\u{222e}", true),
        ("Copf", "\u{2102}", true),
        ("Coproduct", "\u{2210}", true),
        ("CounterClockwiseContourIntegral", "\u{2233}", true),
        ("Cross", "\u{2a2f}", true),
        ("Cscr", "\u{1d49e}", true),
        ("Cup", "\u{22d3}", true),
        ("CupCap", "\u{224d}", true),
        ("DD", "\u{2145}", true),
        ("DDotrahd", "\u{2911}", true),
        ("DJcy", "\u{402}", true),
        ("DScy", "\u{405}", true),
        ("DZcy", "\u{40f}", true),
        ("Dagger", "\u{2021}", true),
        ("Darr", "\u{21a1}", true),
        ("Dashv", "\u{2ae4}", true),
        ("Dcaron", "\u{10e}", true),
        ("Dcy", "\u{414}", true),
        ("Del", "\u{2207}", true),
        ("Delta", "\u{394}", true),
        ("Dfr", "\u{1d507}", true),
        ("DiacriticalAcute", "\u{b4}", true),
        ("DiacriticalDot", "\u{2d9}", true),
        ("DiacriticalDoubleAcute", "\u{2dd}", true),
        ("DiacriticalGrave", "\u{60}", true),
        ("DiacriticalTilde", "\u{2dc}", true),
        ("Diamond", "\u{22c4}", true),
        ("DifferentialD", "\u{2146}", true),
        ("Dopf", "\u{1d53b}", true),
        ("Dot", "\u{a8}", true),
        ("DotDot", "\u{20dc}", true),
        ("DotEqual", "\u{2250}", true),
        ("DoubleContourIntegral", "\u{222f}", true),
        ("DoubleDot", "\u{a8}", true),
        ("DoubleDownArrow", "\u{21d3}", true),
        ("DoubleLeftArrow", "\u{21d0}", true),
        ("DoubleLeftRightArrow", "\u{21d4}", true),
        ("DoubleLeftTee", "\u{2ae4}", true),
        ("DoubleLongLeftArrow", "\u{27f8}", true),
        ("DoubleLongLeftRightArrow", "\u{27fa}", true),
        ("DoubleLongRightArrow", "\u{27f9}", true),
        ("DoubleRightArrow", "\u{21d2}", true),
        ("DoubleRightTee", "\u{22a8}", true),
        ("DoubleUpArrow", "\u{21d1}", true),
        ("DoubleUpDownArrow", "\u{21d5}", true),
        ("DoubleVerticalBar", "\u{2225}", true),
        ("DownArrow", "\u{2193}", true),
        ("DownArrowBar", "\u{2913}", true),
        ("DownArrowUpArrow", "\u{21f5}", true),
        ("DownBreve", "\u{311}", true),
        ("DownLeftRightVector", "\u{2950}", true),
        ("DownLeftTeeVector", "\u{295e}", true),
        ("DownLeftVector", "\u{21bd}", true),
        ("DownLeftVectorBar", "\u{2956}", true),
        ("DownRightTeeVector", "\u{295f}", true),
        ("DownRightVector", "\u{21c1}", true),
        ("DownRightVectorBar", "\u{2957}", true),
        ("DownTee", "\u{22a4}", true),
        ("DownTeeArrow", "\u{21a7}", true),
        ("Downarrow", "\u{21d3}", true),
        ("Dscr", "\u{1d49f}", true),
        ("Dstrok", "\u{110}", true),
        ("ENG", "\u{14a}", true),
        ("ETH", "\u{d0}", false),
        ("Eacute", "\u{c9}", false),
        ("Ecaron", "\u{11a}", true),
        ("Ecirc", "\u{ca}", false),
        ("Ecy", "\u{42d}", true),
        ("Edot", "\u{116}", true),
        ("Efr", "\u{1d508}", true),
        ("Egrave", "\u{c8}", false),
        ("Element", "\u{2208}", true),
        ("Emacr", "\u{112}", true),
        ("EmptySmallSquare", "\u{25fb}", true),
        ("EmptyVerySmallSquare", "\u{25ab}", true),
        ("Eogon", "\u{118}", true),
        ("Eopf", "\u{1d53c}", true),
        ("Epsilon", "\u{395}", true),
        ("Equal", "\u{2a75}", true),
        ("EqualTilde", "\u{2242}", true),
        ("Equilibrium", "\u{21cc}", true),
        ("Escr", "\u{2130}", true),
        ("Esim", "\u{2a73}", true),
        ("Eta", "\u{397}", true),
        ("Euml", "\u{cb}", false),
        ("Exists", "\u{2203}", true),
        ("ExponentialE", "\u{2147}", true),
        ("Fcy", "\u{424}", true),
        ("Ffr", "\u{1d509}", true),
        ("FilledSmallSquare", "\u{25fc}", true),
        ("FilledVerySmallSquare", "\u{25aa}", true),
        ("Fopf", "\u{1d53d}", true),
        ("ForAll", "\u{2200}", true),
        ("Fouriertrf", "\u{2131}", true),
        ("Fscr", "\u{2131}", true),
        ("GJcy", "\u{403}", true),
        ("GT", "\u{3e}", false),
        ("Gamma", "\u{393}", true),
        ("Gammad", "\u{3dc}", true),
        ("Gbreve", "\u{11e}", true),
        ("Gcedil", "\u{122}", true),
        ("Gcirc", "\u{11c}", true),
        ("Gcy", "\u{413}", true),
        ("Gdot", "\u{120}", true),
        ("Gfr", "\u{1d50a}", true),
        ("Gg", "\u{22d9}", true),
        ("Gopf", "\u{1d53e}", true),
        ("GreaterEqual", "\u{2265}", true),
        ("GreaterEqualLess", "\u{22db}", true),
        ("GreaterFullEqual", "\u{2267}", true),
        ("GreaterGreater", "\u{2aa2}", true),
        ("GreaterLess", "\u{2277}", true),
        ("GreaterSlantEqual", "\u{2a7e}", true),
        ("GreaterTilde", "\u{2273}", true),
        ("Gscr", "\u{1d4a2}", true),
        ("Gt", "\u{226b}", true),
        ("HARDcy", "\u{42a}", true),
        ("Hacek", "\u{2c7}", true),
        ("Hat", "\u{5e}", true),
        ("Hcirc", "\u{124}", true),
        ("Hfr", "\u{210c}", true),
        ("HilbertSpace", "\u{210b}", true),
        ("Hopf", "\u{210d}", true),
        ("HorizontalLine", "\u{2500}", true),
        ("Hscr", "\u{210b}", true),
        ("Hstrok", "\u{126}", true),
        ("HumpDownHump", "\u{224e}", true),
        ("HumpEqual", "\u{224f}", true),
        ("IEcy", "\u{415}", true),
        ("IJlig", "\u{132}", true),
        ("IOcy", "\u{401}", true),
        ("Iacute", "\u{cd}", false),
        ("Icirc", "\u{ce}", false),
        ("Icy", "\u{418}", true),
        ("Idot", "\u{130}", true),
        ("Ifr", "\u{2111}", true),
        ("Igrave", "\u{cc}", false),
        ("Im", "\u{2111}", true),
        ("Imacr", "\u{12a}", true),
        ("ImaginaryI", "\u{2148}", true),
        ("Implies", "\u{21d2}", true),
        ("Int", "\u{222c}", true),
        ("Integral", "\u{222b}", true),
        ("Intersection", "\u{22c2}", true),
        ("InvisibleComma", "\u{2063}", true),
        ("InvisibleTimes", "\u{2062}", true),
        ("Iogon", "\u{12e}", true),
        ("Iopf", "\u{1d540}", true),
        ("Iota", "\u{399}", true),
        ("Iscr", "\u{2110}", true),
        ("Itilde", "\u{128}", true),
        ("Iukcy", "\u{406}", true),
        ("Iuml", "\u{cf}", false),
        ("Jcirc", "\u{134}", true),
        ("Jcy", "\u{419}", true),
        ("Jfr", "\u{1d50d}", true),
        ("Jopf", "\u{1d541}", true),
        ("Jscr", "\u{1d4a5}", true),
        ("Jsercy", "\u{408}", true),
        ("Jukcy", "\u{404}", true),
        ("KHcy", "\u{425}", true),
        ("KJcy", "\u{40c}", true),
        ("Kappa", "\u{39a}", true),
        ("Kcedil", "\u{136}", true),
        ("Kcy", "\u{41a}", true),
        ("Kfr", "\u{1d50e}", true),
        ("Kopf", "\u{1d542}", true),
        ("Kscr", "\u{1d4a6}", true),
        ("LJcy", "\u{409}", true),
        ("LT", "\u{3c}", false),
        ("Lacute", "\u{139}", true),
        ("Lambda", "\u{39b}", true),
        ("Lang", "\u{27ea}", true),
        ("Laplacetrf", "\u{2112}", true),
        ("Larr", "\u{219e}", true),
        ("Lcaron", "\u{13d}", true),
        ("Lcedil", "\u{13b}", true),
        ("Lcy", "\u{41b}", true),
        ("LeftAngleBracket", "\u{27e8}", true),
        ("LeftArrow", "\u{2190}", true),
        ("LeftArrowBar", "\u{21e4}", true),
        ("LeftArrowRightArrow", "\u{21c6}", true),
        ("LeftCeiling", "\u{2308}", true),
        ("LeftDoubleBracket", "\u{27e6}", true),
        ("LeftDownTeeVector", "\u{2961}", true),
        ("LeftDownVector", "\u{21c3}", true),
        ("LeftDownVectorBar", "\u{2959}", true),
        ("LeftFloor", "\u{230a}", true),
        ("LeftRightArrow", "\u{2194}", true),
        ("LeftRightVector", "\u{294e}", true),
        ("LeftTee", "\u{22a3}", true),
        ("LeftTeeArrow", "\u{21a4}", true),
        ("LeftTeeVector", "\u{295a}", true),
        ("LeftTriangle", "\u{22b2}", true),
        ("LeftTriangleBar", "\u{29cf}", true),
        ("LeftTriangleEqual", "\u{22b4}", true),
        ("LeftUpDownVector", "\u{2951}", true),
        ("LeftUpTeeVector", "\u{2960}", true),
        ("LeftUpVector", "\u{21bf}", true),
        ("LeftUpVectorBar", "\u{2958}", true),
        ("LeftVector", "\u{21bc}", true),
        ("LeftVectorBar", "\u{2952}", true),
        ("Leftarrow", "\u{21d0}", true),
        ("Leftrightarrow", "\u{21d4}", true),
        ("LessEqualGreater", "\u{22da}", true),
        ("LessFullEqual", "\u{2266}", true),
        ("LessGreater", "\u{2276}", true),
        ("LessLess", "\u{2aa1}", true),
        ("LessSlantEqual", "\u{2a7d}", true),
        ("LessTilde", "\u{2272}", true),
        ("Lfr", "\u{1d50f}", true),
        ("Ll", "\u{22d8}", true),
        ("Lleftarrow", "\u{21da}", true),
        ("Lmidot", "\u{13f}", true),
        ("LongLeftArrow", "\u{27f5}", true),
        ("LongLeftRightArrow", "\u{27f7}", true),
        ("LongRightArrow", "\u{27f6}", true),
        ("Longleftarrow", "\u{27f8}", true),
        ("Longleftrightarrow", "\u{27fa}", true),
        ("Longrightarrow", "\u{27f9}", true),
        ("Lopf", "\u{1d543}", true),
        ("LowerLeftArrow", "\u{2199}", true),
        ("LowerRightArrow", "\u{2198}", true),
        ("Lscr", "\u{2112}", true),
        ("Lsh", "\u{21b0}", true),
        ("Lstrok", "\u{141}", true),
        ("Lt", "\u{226a}", true),
        ("Map", "\u{2905}", true),
        ("Mcy", "\u{41c}", true),
        ("MediumSpace", "\u{205f}", true),
        ("Mellintrf", "\u{2133}", true),
        ("Mfr", "\u{1d510}", true),
        ("MinusPlus", "\u{2213}", true),
        ("Mopf", "\u{1d544}", true),
        ("Mscr", "\u{2133}", true),
        ("Mu", "\u{39c}", true),
        ("NJcy", "\u{40a}", true),
        ("Nacute", "\u{143}", true),
        ("Ncaron", "\u{147}", true),
        ("Ncedil", "\u{145}", true),
        ("Ncy", "\u{41d}", true),
        ("NegativeMediumSpace", "\u{200b}", true),
        ("NegativeThickSpace", "\u{200b}", true),
        ("NegativeThinSpace", "\u{200b}", true),
        ("NegativeVeryThinSpace", "\u{200b}", true),
        ("NestedGreaterGreater", "\u{226b}", true),
        ("NestedLessLess", "\u{226a}", true),
        ("NewLine", "\u{a}", true),
        ("Nfr", "\u{1d511}", true),
        ("NoBreak", "\u{2060}", true),
        ("NonBreakingSpace", "\u{a0}", true),
        ("Nopf", "\u{2115}", true),
        ("Not", "\u{2aec}", true),
        ("NotCongruent", "\u{2262}", true),
        ("NotCupCap", "\u{226d}", true),
        ("NotDoubleVerticalBar", "\u{2226}", true),
        ("NotElement", "\u{2209}", true),
        ("NotEqual", "\u{2260}", true),
        ("NotEqualTilde", "\u{2242}\u{338}", true),
        ("NotExists", "\u{2204}", true),
        ("NotGreater", "\u{226f}", true),
        ("NotGreaterEqual", "\u{2271}", true),
        ("NotGreaterFullEqual", "\u{2267}\u{338}", true),
        ("NotGreaterGreater", "\u{226b}\u{338}", true),
        ("NotGreaterLess", "\u{2279}", true),
        ("NotGreaterSlantEqual", "\u{2a7e}\u{338}", true),
        ("NotGreaterTilde", "\u{2275}", true),
        ("NotHumpDownHump", "\u{224e}\u{338}", true),
        ("NotHumpEqual", "\u{224f}\u{338}", true),
        ("NotLeftTriangle", "\u{22ea}", true),
        ("NotLeftTriangleBar", "\u{29cf}\u{338}", true),
        ("NotLeftTriangleEqual", "\u{22ec}", true),
        ("NotLess", "\u{226e}", true),
        ("NotLessEqual", "\u{2270}", true),
        ("NotLessGreater", "\u{2278}", true),
        ("NotLessLess", "\u{226a}\u{338}", true),
        ("NotLessSlantEqual", "\u{2a7d}\u{338}", true),
        ("NotLessTilde", "\u{2274}", true),
        ("NotNestedGreaterGreater", "\u{2aa2}\u{338}", true),
        ("NotNestedLessLess", "\u{2aa1}\u{338}", true),
        ("NotPrecedes", "\u{2280}", true),
        ("NotPrecedesEqual", "\u{2aaf}\u{338}", true),
        ("NotPrecedesSlantEqual", "\u{22e0}", true),
        ("NotReverseElement", "\u{220c}", true),
        ("NotRightTriangle", "\u{22eb}", true),
        ("NotRightTriangleBar", "\u{29d0}\u{338}", true),
        ("NotRightTriangleEqual", "\u{22ed}", true),
        ("NotSquareSubset", "\u{228f}\u{338}", true),
        ("NotSquareSubsetEqual", "\u{22e2}", true),
        ("NotSquareSuperset", "\u{2290}\u{338}", true),
        ("NotSquareSupersetEqual", "\u{22e3}", true),
        ("NotSubset", "\u{2282}\u{20d2}", true),
        ("NotSubsetEqual", "\u{2288}", true),
        ("NotSucceeds", "\u{2281}", true),
        ("NotSucceedsEqual", "\u{2ab0}\u{338}", true),
        ("NotSucceedsSlantEqual", "\u{22e1}", true),
        ("NotSucceedsTilde", "\u{227f}\u{338}", true),
        ("NotSuperset", "\u{2283}\u{20d2}", true),
        ("NotSupersetEqual", "\u{2289}", true),
        ("NotTilde", "\u{2241}", true),
        ("NotTildeEqual", "\u{2244}", true),
        ("NotTildeFullEqual", "\u{2247}", true),
        ("NotTildeTilde", "\u{2249}", true),
        ("NotVerticalBar", "\u{2224}", true),
        ("Nscr", "\u{1d4a9}", true),
        ("Ntilde", "\u{d1}", false),
        ("Nu", "\u{39d}", true),
        ("OElig", "\u{152}", true),
        ("Oacute", "\u{d3}", false),
        ("Ocirc", "\u{d4}", false),
        ("Ocy", "\u{41e}", true),
        ("Odblac", "\u{150}", true),
        ("Ofr", "\u{1d512}", true),
        ("Ograve", "\u{d2}", false),
        ("Omacr", "\u{14c}", true),
        ("Omega", "\u{3a9}", true),
        ("Omicron", "\u{39f}", true),
        ("Oopf", "\u{1d546}", true),
        ("OpenCurlyDoubleQuote", "\u{201c}", true),
        ("OpenCurlyQuote", "\u{2018}", true),
        ("Or", "\u{2a54}", true),
        ("Oscr", "\u{1d4aa}", true),
        ("Oslash", "\u{d8}", false),
        ("Otilde", "\u{d5}", false),
        ("Otimes", "\u{2a37}", true),
        ("Ouml", "\u{d6}", false),
        ("OverBar", "\u{203e}", true),
        ("OverBrace", "\u{23de}", true),
        ("OverBracket", "\u{23b4}", true),
        ("OverParenthesis", "\u{23dc}", true),
        ("PartialD", "\u{2202}", true),
        ("Pcy", "\u{41f}", true),
        ("Pfr", "\u{1d513}", true),
        ("Phi", "\u{3a6}", true),
        ("Pi", "\u{3a0}", true),
        ("PlusMinus", "\u{b1}", true),
        ("Poincareplane", "\u{210c}", true),
        ("Popf", "\u{2119}", true),
        ("Pr", "\u{2abb}", true),
        ("Precedes", "\u{227a}", true),
        ("PrecedesEqual", "\u{2aaf}", true),
        ("PrecedesSlantEqual", "\u{227c}", true),
        ("PrecedesTilde", "\u{227e}", true),
        ("Prime", "\u{2033}", true),
        ("Product", "\u{220f}", true),
        ("Proportion", "\u{2237}", true),
        ("Proportional", "\u{221d}", true),
        ("Pscr", "\u{1d4ab}", true),
        ("Psi", "\u{3a8}", true),
        ("QUOT", "\u{22}", false),
        ("Qfr", "\u{1d514}", true),
        ("Qopf", "\u{211a}", true),
        ("Qscr", "\u{1d4ac}", true),
        ("RBarr", "\u{2910}", true),
        ("REG", "\u{ae}", false),
        ("Racute", "\u{154}", true),
        ("Rang", "\u{27eb}", true),
        ("Rarr", "\u{21a0}", true),
        ("Rarrtl", "\u{2916}", true),
        ("Rcaron", "\u{158}", true),
        ("Rcedil", "\u{156}", true),
        ("Rcy", "\u{420}", true),
        ("Re", "\u{211c}", true),
        ("ReverseElement", "\u{220b}", true),
        ("ReverseEquilibrium", "\u{21cb}", true),
        ("ReverseUpEquilibrium", "\u{296f}", true),
        ("Rfr", "\u{211c}", true),
        ("Rho", "\u{3a1}", true),
        ("RightAngleBracket", "\u{27e9}", true),
        ("RightArrow", "\u{2192}", true),
        ("RightArrowBar", "\u{21e5}", true),
        ("RightArrowLeftArrow", "\u{21c4}", true),
        ("RightCeiling", "\u{2309}", true),
        ("RightDoubleBracket", "\u{27e7}", true),
        ("RightDownTeeVector", "\u{295d}", true),
        ("RightDownVector", "\u{21c2}", true),
        ("RightDownVectorBar", "\u{2955}", true),
        ("RightFloor", "\u{230b}", true),
        ("RightTee", "\u{22a2}", true),
        ("RightTeeArrow", "\u{21a6}", true),
        ("RightTeeVector", "\u{295b}", true),
        ("RightTriangle", "\u{22b3}", true),
        ("RightTriangleBar", "\u{29d0}", true),
        ("RightTriangleEqual", "\u{22b5}", true),
        ("RightUpDownVector", "\u{294f}", true),
        ("RightUpTeeVector", "\u{295c}", true),
        ("RightUpVector", "\u{21be}", true),
        ("RightUpVectorBar", "\u{2954}", true),
        ("RightVector", "\u{21c0}", true),
        ("RightVectorBar", "\u{2953}", true),
        ("Rightarrow", "\u{21d2}", true),
        ("Ropf", "\u{211d}", true),
        ("RoundImplies", "\u{2970}", true),
        ("Rrightarrow", "\u{21db}", true),
        ("Rscr", "\u{211b}", true),
        ("Rsh", "\u{21b1}", true),
        ("RuleDelayed", "\u{29f4}", true),
        ("SHCHcy", "\u{429}", true),
        ("SHcy", "\u{428}", true),
        ("SOFTcy", "\u{42c}", true),
        ("Sacute", "\u{15a}", true),
        ("Sc", "\u{2abc}", true),
        ("Scaron", "\u{160}", true),
        ("Scedil", "\u{15e}", true),
        ("Scirc", "\u{15c}", true),
        ("Scy", "\u{421}", true),
        ("Sfr", "\u{1d516}", true),
        ("ShortDownArrow", "\u{2193}", true),
        ("ShortLeftArrow", "\u{2190}", true),
        ("ShortRightArrow", "\u{2192}", true),
        ("ShortUpArrow", "\u{2191}", true),
        ("Sigma", "\u{3a3}", true),
        ("SmallCircle", "\u{2218}", true),
        ("Sopf", "\u{1d54a}", true),
        ("Sqrt", "\u{221a}", true),
        ("Square", "\u{25a1}", true),
        ("SquareIntersection", "\u{2293}", true),
        ("SquareSubset", "\u{228f}", true),
        ("SquareSubsetEqual", "\u{2291}", true),
        ("SquareSuperset", "\u{2290}", true),
        ("SquareSupersetEqual", "\u{2292}", true),
        ("SquareUnion", "\u{2294}", true),
        ("Sscr", "\u{1d4ae}", true),
        ("Star", "\u{22c6}", true),
        ("Sub", "\u{22d0}", true),
        ("Subset", "\u{22d0}", true),
        ("SubsetEqual", "\u{2286}", true),
        ("Succeeds", "\u{227b}", true),
        ("SucceedsEqual", "\u{2ab0}", true),
        ("SucceedsSlantEqual", "\u{227d}", true),
        ("SucceedsTilde", "\u{227f}", true),
        ("SuchThat", "\u{220b}", true),
        ("Sum", "\u{2211}", true),
        ("Sup", "\u{22d1}", true),
        ("Superset", "\u{2283}", true),
        ("SupersetEqual", "\u{2287}", true),
        ("Supset", "\u{22d1}", true),
        ("THORN", "\u{de}", false),
        ("TRADE", "\u{2122}", true),
        ("TSHcy", "\u{40b}", true),
        ("TScy", "\u{426}", true),
        ("Tab", "\u{9}", true),
        ("Tau", "\u{3a4}", true),
        ("Tcaron", "\u{164}", true),
        ("Tcedil", "\u{162}", true),
        ("Tcy", "\u{422}", true),
        ("Tfr", "\u{1d517}", true),
        ("Therefore", "\u{2234}", true),
        ("Theta", "\u{398}", true),
        ("ThickSpace", "\u{205f}\u{200a}", true),
        ("ThinSpace", "\u{2009}", true),
        ("Tilde", "\u{223c}", true),
        ("TildeEqual", "\u{2243}", true),
        ("TildeFullEqual", "\u{2245}", true),
        ("TildeTilde", "\u{2248}", true),
        ("Topf", "\u{1d54b}", true),
        ("TripleDot", "\u{20db}", true),
        ("Tscr", "\u{1d4af}", true),
        ("Tstrok", "\u{166}", true),
        ("Uacute", "\u{da}", false),
        ("Uarr", "\u{219f}", true),
        ("Uarrocir", "\u{2949}", true),
        ("Ubrcy", "\u{40e}", true),
        ("Ubreve", "\u{16c}", true),
        ("Ucirc", "\u{db}", false),
        ("Ucy", "\u{423}", true),
        ("Udblac", "\u{170}", true),
        ("Ufr", "\u{1d518}", true),
        ("Ugrave", "\u{d9}", false),
        ("Umacr", "\u{16a}", true),
        ("UnderBar", "\u{5f}", true),
        ("UnderBrace", "\u{23df}", true),
        ("UnderBracket", "\u{23b5}", true),
        ("UnderParenthesis", "\u{23dd}", true),
        ("Union", "\u{22c3}", true),
        ("UnionPlus", "\u{228e}", true),
        ("Uogon", "\u{172}", true),
        ("Uopf", "\u{1d54c}", true),
        ("UpArrow", "\u{2191}", true),
        ("UpArrowBar", "\u{2912}", true),
        ("UpArrowDownArrow", "\u{21c5}", true),
        ("UpDownArrow", "\u{2195}", true),
        ("UpEquilibrium", "\u{296e}", true),
        ("UpTee", "\u{22a5}", true),
        ("UpTeeArrow", "\u{21a5}", true),
        ("Uparrow", "\u{21d1}", true),
        ("Updownarrow", "\u{21d5}", true),
        ("UpperLeftArrow", "\u{2196}", true),
        ("UpperRightArrow", "\u{2197}", true),
        ("Upsi", "\u{3d2}", true),
        ("Upsilon", "\u{3a5}", true),
        ("Uring", "\u{16e}", true),
        ("Uscr", "\u{1d4b0}", true),
        ("Utilde", "\u{168}", true),
        ("Uuml", "\u{dc}", false),
        ("VDash", "\u{22ab}", true),
        ("Vbar", "\u{2aeb}", true),
        ("Vcy", "\u{412}", true),
        ("Vdash", "\u{22a9}", true),
        ("Vdashl", "\u{2ae6}", true),
        ("Vee", "\u{22c1}", true),
        ("Verbar", "\u{2016}", true),
        ("Vert", "\u{2016}", true),
        ("VerticalBar", "\u{2223}", true),
        ("VerticalLine", "\u{7c}", true),
        ("VerticalSeparator", "\u{2758}", true),
        ("VerticalTilde", "\u{2240}", true),
        ("VeryThinSpace", "\u{200a}", true),
        ("Vfr", "\u{1d519}", true),
        ("Vopf", "\u{1d54d}", true),
        ("Vscr", "\u{1d4b1}", true),
        ("Vvdash", "\u{22aa}", true),
        ("Wcirc", "\u{174}", true),
        ("Wedge", "\u{22c0}", true),
        ("Wfr", "\u{1d51a}", true),
        ("Wopf", "\u{1d54e}", true),
        ("Wscr", "\u{1d4b2}", true),
        ("Xfr", "\u{1d51b}", true),
        ("Xi", "\u{39e}", true),
        ("Xopf", "\u{1d54f}", true),
        ("Xscr", "\u{1d4b3}", true),
        ("YAcy", "\u{42f}", true),
        ("YIcy", "\u{407}", true),
        ("YUcy", "\u{42e}", true),
        ("Yacute", "\u{dd}", false),
        ("Ycirc", "\u{176}", true),
        ("Ycy", "\u{42b}", true),
        ("Yfr", "\u{1d51c}", true),
        ("Yopf", "\u{1d550}", true),
        ("Yscr", "\u{1d4b4}", true),
        ("Yuml", "\u{178}", true),
        ("ZHcy", "\u{416}", true),
        ("Zacute", "\u{179}", true),
        ("Zcaron", "\u{17d}", true),
        ("Zcy", "\u{417}", true),
        ("Zdot", "\u{17b}", true),
        ("ZeroWidthSpace", "\u{200b}", true),
        ("Zeta", "\u{396}", true),
        ("Zfr", "\u{2128}", true),
        ("Zopf", "\u{2124}", true),
        ("Zscr", "\u{1d4b5}", true),
        ("aacute", "\u{e1}", false),
        ("abreve", "\u{103}", true),
        ("ac", "\u{223e}", true),
        ("acE", "\u{223e}\u{333}", true),
        ("acd", "\u{223f}", true),
        ("acirc", "\u{e2}", false),
        ("acute", "\u{b4}", false),
        ("acy", "\u{430}", true),
        ("aelig", "\u{e6}", false),
        ("af", "\u{2061}", true),
        ("afr", "\u{1d51e}", true),
        ("agrave", "\u{e0}", false),
        ("alefsym", "\u{2135}", true),
        ("aleph", "\u{2135}", true),
        ("alpha", "\u{3b1}", true),
        ("amacr", "\u{101}", true),
        ("amalg", "\u{2a3f}", true),
        ("amp", "\u{26}", false),
        ("and", "\u{2227}", true),
        ("andand", "\u{2a55}", true),
        ("andd", "\u{2a5c}", true),
        ("andslope", "\u{2a58}", true),
        ("andv", "\u{2a5a}", true),
        ("ang", "\u{2220}", true),
        ("ange", "\u{29a4}", true),
        ("angle", "\u{2220}", true),
        ("angmsd", "\u{2221}", true),
        ("angmsdaa", "\u{29a8}", true),
        ("angmsdab", "\u{29a9}", true),
        ("angmsdac", "\u{29aa}", true),
        ("angmsdad", "\u{29ab}", true),
        ("angmsdae", "\u{29ac}", true),
        ("angmsdaf", "\u{29ad}", true),
        ("angmsdag", "\u{29ae}", true),
        ("angmsdah", "\u{29af}", true),
        ("angrt", "\u{221f}", true),
        ("angrtvb", "\u{22be}", true),
        ("angrtvbd", "\u{299d}", true),
        ("angsph", "\u{2222}", true),
        ("angst", "\u{c5}", true),
        ("angzarr", "\u{237c}", true),
        ("aogon", "\u{105}", true),
        ("aopf", "\u{1d552}", true),
        ("ap", "\u{2248}", true),
        ("apE", "\u{2a70}", true),
        ("apacir", "\u{2a6f}", true),
        ("ape", "\u{224a}", true),
        ("apid", "\u{224b}", true),
        ("apos", "\u{27}", true),
        ("approx", "\u{2248}", true),
        ("approxeq", "\u{224a}", true),
        ("aring", "\u{e5}", false),
        ("ascr", "\u{1d4b6}", true),
        ("ast", "\u{2a}", true),
        ("asymp", "\u{2248}", true),
        ("asympeq", "\u{224d}", true),
        ("atilde", "\u{e3}", false),
        ("auml", "\u{e4}", false),
        ("awconint", "\u{2233}", true),
        ("awint", "\u{2a11}", true),
        ("bNot", "\u{2aed}", true),
        ("backcong", "\u{224c}", true),
        ("backepsilon", "\u{3f6}", true),
        ("backprime", "\u{2035}", true),
        ("backsim", "\u{223d}", true),
        ("backsimeq", "\u{22cd}", true),
        ("barvee", "\u{22bd}", true),
        ("barwed", "\u{2305}", true),
        ("barwedge", "\u{2305}", true),
        ("bbrk", "\u{23b5}", true),
        ("bbrktbrk", "\u{23b6}", true),
        ("bcong", "\u{224c}", true),
        ("bcy", "\u{431}", true),
        ("bdquo", "\u{201e}", true),
        ("becaus", "\u{2235}", true),
        ("because", "\u{2235}", true),
        ("bemptyv", "\u{29b0}", true),
        ("bepsi", "\u{3f6}", true),
        ("bernou", "\u{212c}", true),
        ("beta", "\u{3b2}", true),
        ("beth", "\u{2136}", true),
        ("between", "\u{226c}", true),
        ("bfr", "\u{1d51f}", true),
        ("bigcap", "\u{22c2}", true),
        ("bigcirc", "\u{25ef}", true),
        ("bigcup", "\u{22c3}", true),
        ("bigodot", "\u{2a00}", true),
        ("bigoplus", "\u{2a01}", true),
        ("bigotimes", "\u{2a02}", true),
        ("bigsqcup", "\u{2a06}", true),
        ("bigstar", "\u{2605}", true),
        ("bigtriangledown", "\u{25bd}", true),
        ("bigtriangleup", "\u{25b3}", true),
        ("biguplus", "\u{2a04}", true),
        ("bigvee", "\u{22c1}", true),
        ("bigwedge", "\u{22c0}", true),
        ("bkarow", "\u{290d}", true),
        ("blacklozenge", "\u{29eb}", true),
        ("blacksquare", "\u{25aa}", true),
        ("blacktriangle", "\u{25b4}", true),
        ("blacktriangledown", "\u{25be}", true),
        ("blacktriangleleft", "\u{25c2}", true),
        ("blacktriangleright", "\u{25b8}", true),
        ("blank", "\u{2423}", true),
        ("blk12", "\u{2592}", true),
        ("blk14", "\u{2591}", true),
        ("blk34", "\u{2593}", true),
        ("block", "\u{2588}", true),
        ("bne", "\u{3d}\u{20e5}", true),
        ("bnequiv", "\u{2261}\u{20e5}", true),
        ("bnot", "\u{2310}", true),
        ("bopf", "\u{1d553}", true),
        ("bot", "\u{22a5}", true),
        ("bottom", "\u{22a5}", true),
        ("bowtie", "\u{22c8}", true),
        ("boxDL", "\u{2557}", true),
        ("boxDR", "\u{2554}", true),
        ("boxDl", "\u{2556}", true),
        ("boxDr", "\u{2553}", true),
        ("boxH", "\u{2550}", true),
        ("boxHD", "\u{2566}", true),
        ("boxHU", "\u{2569}", true),
        ("boxHd", "\u{2564}", true),
        ("boxHu", "\u{2567}", true),
        ("boxUL", "\u{255d}", true),
        ("boxUR", "\u{255a}", true),
        ("boxUl", "\u{255c}", true),
        ("boxUr", "\u{2559}", true),
        ("boxV", "\u{2551}", true),
        ("boxVH", "\u{256c}", true),
        ("boxVL", "\u{2563}", true),
        ("boxVR", "\u{2560}", true),
        ("boxVh", "\u{256b}", true),
        ("boxVl", "\u{2562}", true),
        ("boxVr", "\u{255f}", true),
        ("boxbox", "\u{29c9}", true),
        ("boxdL", "\u{2555}", true),
        ("boxdR", "\u{2552}", true),
        ("boxdl", "\u{2510}", true),
        ("boxdr", "\u{250c}", true),
        ("boxh", "\u{2500}", true),
        ("boxhD", "\u{2565}", true),
        ("boxhU", "\u{2568}", true),
        ("boxhd", "\u{252c}", true),
        ("boxhu", "\u{2534}", true),
        ("boxminus", "\u{229f}", true),
        ("boxplus", "\u{229e}", true),
        ("boxtimes", "\u{22a0}", true),
        ("boxuL", "\u{255b}", true),
        ("boxuR", "\u{2558}", true),
        ("boxul", "\u{2518}", true),
        ("boxur", "\u{2514}", true),
        ("boxv", "\u{2502}", true),
        ("boxvH", "\u{256a}", true),
        ("boxvL", "\u{2561}", true),
        ("boxvR", "\u{255e}", true),
        ("boxvh", "\u{253c}", true),
        ("boxvl", "\u{2524}", true),
        ("boxvr", "\u{251c}", true),
        ("bprime", "\u{2035}", true),
        ("breve", "\u{2d8}", true),
        ("brvbar", "\u{a6}", false),
        ("bscr", "\u{1d4b7}", true),
        ("bsemi", "\u{204f}", true),
        ("bsim", "\u{223d}", true),
        ("bsime", "\u{22cd}", true),
        ("bsol", "\u{5c}", true),
        ("bsolb", "\u{29c5}", true),
        ("bsolhsub", "\u{27c8}", true),
        ("bull", "\u{2022}", true),
        ("bullet", "\u{2022}", true),
        ("bump", "\u{224e}", true),
        ("bumpE", "\u{2aae}", true),
        ("bumpe", "\u{224f}", true),
        ("bumpeq", "\u{224f}", true),
        ("cacute", "\u{107}", true),
        ("cap", "\u{2229}", true),
        ("capand", "\u{2a44}", true),
        ("capbrcup", "\u{2a49}", true),
        ("capcap", "\u{2a4b}", true),
        ("capcup", "\u{2a47}", true),
        ("capdot", "\u{2a40}", true),
        ("caps", "\u{2229}\u{fe00}", true),
        ("caret", "\u{2041}", true),
        ("caron", "\u{2c7}", true),
        ("ccaps", "\u{2a4d}", true),
        ("ccaron", "\u{10d}", true),
        ("ccedil", "\u{e7}", false),
        ("ccirc", "\u{109}", true),
        ("ccups", "\u{2a4c}", true),
        ("ccupssm", "\u{2a50}", true),
        ("cdot", "\u{10b}", true),
        ("cedil", "\u{b8}", false),
        ("cemptyv", "\u{29b2}", true),
        ("cent", "\u{a2}", false),
        ("centerdot", "\u{b7}", true),
        ("cfr", "\u{1d520}", true),
        ("chcy", "\u{447}", true),
        ("check", "\u{2713}", true),
        ("checkmark", "\u{2713}", true),
        ("chi", "\u{3c7}", true),
        ("cir", "\u{25cb}", true),
        ("cirE", "\u{29c3}", true),
        ("circ", "\u{2c6}", true),
        ("circeq", "\u{2257}", true),
        ("circlearrowleft", "\u{21ba}", true),
        ("circlearrowright", "\u{21bb}", true),
        ("circledR", "\u{ae}", true),
        ("circledS", "\u{24c8}", true),
        ("circledast", "\u{229b}", true),
        ("circledcirc", "\u{229a}", true),
        ("circleddash", "\u{229d}", true),
        ("cire", "\u{2257}", true),
        ("cirfnint", "\u{2a10}", true),
        ("cirmid", "\u{2aef}", true),
        ("cirscir", "\u{29c2}", true),
        ("clubs", "\u{2663}", true),
        ("clubsuit", "\u{2663}", true),
        ("colon", "\u{3a}", true),
        ("colone", "\u{2254}", true),
        ("coloneq", "\u{2254}", true),
        ("comma", "\u{2c}", true),
        ("commat", "\u{40}", true),
        ("comp", "\u{2201}", true),
        ("compfn", "\u{2218}", true),
        ("complement", "\u{2201}", true),
        ("complexes", "\u{2102}", true),
        ("cong", "\u{2245}", true),
        ("congdot", "\u{2a6d}", true),
        ("conint", "\u{222e}", true),
        ("copf", "\u{1d554}", true),
        ("coprod", "\u{2210}", true),
        ("copy", "\u{a9}", false),
        ("copysr", "\u{2117}", true),
        ("crarr", "\u{21b5}", true),
        ("cross", "\u{2717}", true),
        ("cscr", "\u{1d4b8}", true),
        ("csub", "\u{2acf}", true),
        ("csube", "\u{2ad1}", true),
        ("csup", "\u{2ad0}", true),
        ("csupe", "\u{2ad2}", true),
        ("ctdot", "\u{22ef}", true),
        ("cudarrl", "\u{2938}", true),
        ("cudarrr", "\u{2935}", true),
        ("cuepr", "\u{22de}", true),
        ("cuesc", "\u{22df}", true),
        ("cularr", "\u{21b6}", true),
        ("cularrp", "\u{293d}", true),
        ("cup", "\u{222a}", true),
        ("cupbrcap", "\u{2a48}", true),
        ("cupcap", "\u{2a46}", true),
        ("cupcup", "\u{2a4a}", true),
        ("cupdot", "\u{228d}", true),
        ("cupor", "\u{2a45}", true),
        ("cups", "\u{222a}\u{fe00}", true),
        ("curarr", "\u{21b7}", true),
        ("curarrm", "\u{293c}", true),
        ("curlyeqprec", "\u{22de}", true),
        ("curlyeqsucc", "\u{22df}", true),
        ("curlyvee", "\u{22ce}", true),
        ("curlywedge", "\u{22cf}", true),
        ("curren", "\u{a4}", false),
        ("curvearrowleft", "\u{21b6}", true),
        ("curvearrowright", "\u{21b7}", true),
        ("cuvee", "\u{22ce}", true),
        ("cuwed", "\u{22cf}", true),
        ("cwconint", "\u{2232}", true),
        ("cwint", "\u{2231}", true),
        ("cylcty", "\u{232d}", true),
        ("dArr", "\u{21d3}", true),
        ("dHar", "\u{2965}", true),
        ("dagger", "\u{2020}", true),
        ("daleth", "\u{2138}", true),
        ("darr", "\u{2193}", true),
        ("dash", "\u{2010}", true),
        ("dashv", "\u{22a3}", true),
        ("dbkarow", "\u{290f}", true),
        ("dblac", "\u{2dd}", true),
        ("dcaron", "\u{10f}", true),
        ("dcy", "\u{434}", true),
        ("dd", "\u{2146}", true),
        ("ddagger", "\u{2021}", true),
        ("ddarr", "\u{21ca}", true),
        ("ddotseq", "\u{2a77}", true),
        ("deg", "\u{b0}", false),
        ("delta", "\u{3b4}", true),
        ("demptyv", "\u{29b1}", true),
        ("dfisht", "\u{297f}", true),
        ("dfr", "\u{1d521}", true),
        ("dharl", "\u{21c3}", true),
        ("dharr", "\u{21c2}", true),
        ("diam", "\u{22c4}", true),
        ("diamond", "\u{22c4}", true),
        ("diamondsuit", "\u{2666}", true),
        ("diams", "\u{2666}", true),
        ("die", "\u{a8}", true),
        ("digamma", "\u{3dd}", true),
        ("disin", "\u{22f2}", true),
        ("div", "\u{f7}", true),
        ("divide", "\u{f7}", false),
        ("divideontimes", "\u{22c7}", true),
        ("divonx", "\u{22c7}", true),
        ("djcy", "\u{452}", true),
        ("dlcorn", "\u{231e}", true),
        ("dlcrop", "\u{230d}", true),
        ("dollar", "\u{24}", true),
        ("dopf", "\u{1d555}", true),
        ("dot", "\u{2d9}", true),
        ("doteq", "\u{2250}", true),
        ("doteqdot", "\u{2251}", true),
        ("dotminus", "\u{2238}", true),
        ("dotplus", "\u{2214}", true),
        ("dotsquare", "\u{22a1}", true),
        ("doublebarwedge", "\u{2306}", true),
        ("downarrow", "\u{2193}", true),
        ("downdownarrows", "\u{21ca}", true),
        ("downharpoonleft", "\u{21c3}", true),
        ("downharpoonright", "\u{21c2}", true),
        ("drbkarow", "\u{2910}", true),
        ("drcorn", "\u{231f}", true),
        ("drcrop", "\u{230c}", true),
        ("dscr", "\u{1d4b9}", true),
        ("dscy", "\u{455}", true),
        ("dsol", "\u{29f6}", true),
        ("dstrok", "\u{111}", true),
        ("dtdot", "\u{22f1}", true),
        ("dtri", "\u{25bf}", true),
        ("dtrif", "\u{25be}", true),
        ("duarr", "\u{21f5}", true),
        ("duhar", "\u{296f}", true),
        ("dwangle", "\u{29a6}", true),
        ("dzcy", "\u{45f}", true),
        ("dzigrarr", "\u{27ff}", true),
        ("eDDot", "\u{2a77}", true),
        ("eDot", "\u{2251}", true),
        ("eacute", "\u{e9}", false),
        ("easter", "\u{2a6e}", true),
        ("ecaron", "\u{11b}", true),
        ("ecir", "\u{2256}", true),
        ("ecirc", "\u{ea}", false),
        ("ecolon", "\u{2255}", true),
        ("ecy", "\u{44d}", true),
        ("edot", "\u{117}", true),
        ("ee", "\u{2147}", true),
        ("efDot", "\u{2252}", true),
        ("efr", "\u{1d522}", true),
        ("eg", "\u{2a9a}", true),
        ("egrave", "\u{e8}", false),
        ("egs", "\u{2a96}", true),
        ("egsdot", "\u{2a98}", true),
        ("el", "\u{2a99}", true),
        ("elinters", "\u{23e7}", true),
        ("ell", "\u{2113}", true),
        ("els", "\u{2a95}", true),
        ("elsdot", "\u{2a97}", true),
        ("emacr", "\u{113}", true),
        ("empty", "\u{2205}", true),
        ("emptyset", "\u{2205}", true),
        ("emptyv", "\u{2205}", true),
        ("emsp", "\u{2003}", true),
        ("emsp13", "\u{2004}", true),
        ("emsp14", "\u{2005}", true),
        ("eng", "\u{14b}", true),
        ("ensp", "\u{2002}", true),
        ("eogon", "\u{119}", true),
        ("eopf", "\u{1d556}", true),
        ("epar", "\u{22d5}", true),
        ("eparsl", "\u{29e3}", true),
        ("eplus", "\u{2a71}", true),
        ("epsi", "\u{3b5}", true),
        ("epsilon", "\u{3b5}", true),
        ("epsiv", "\u{3f5}", true),
        ("eqcirc", "\u{2256}", true),
        ("eqcolon", "\u{2255}", true),
        ("eqsim", "\u{2242}", true),
        ("eqslantgtr", "\u{2a96}", true),
        ("eqslantless", "\u{2a95}", true),
        ("equals", "\u{3d}", true),
        ("equest", "\u{225f}", true),
        ("equiv", "\u{2261}", true),
        ("equivDD", "\u{2a78}", true),
        ("eqvparsl", "\u{29e5}", true),
        ("erDot", "\u{2253}", true),
        ("erarr", "\u{2971}", true),
        ("escr", "\u{212f}", true),
        ("esdot", "\u{2250}", true),
        ("esim", "\u{2242}", true),
        ("eta", "\u{3b7}", true),
        ("eth", "\u{f0}", false),
        ("euml", "\u{eb}", false),
        ("euro", "\u{20ac}", true),
        ("excl", "\u{21}", true),
        ("exist", "\u{2203}", true),
        ("expectation", "\u{2130}", true),
        ("exponentiale", "\u{2147}", true),
        ("fallingdotseq", "\u{2252}", true),
        ("fcy", "\u{444}", true),
        ("female", "\u{2640}", true),
        ("ffilig", "\u{fb03}", true),
        ("fflig", "\u{fb00}", true),
        ("ffllig", "\u{fb04}", true),
        ("ffr", "\u{1d523}", true),
        ("filig", "\u{fb01}", true),
        ("fjlig", "\u{66}\u{6a}", true),
        ("flat", "\u{266d}", true),
        ("fllig", "\u{fb02}", true),
        ("fltns", "\u{25b1}", true),
        ("fnof", "\u{192}", true),
        ("fopf", "\u{1d557}", true),
        ("forall", "\u{2200}", true),
        ("fork", "\u{22d4}", true),
        ("forkv", "\u{2ad9}", true),
        ("fpartint", "\u{2a0d}", true),
        ("frac12", "\u{bd}", false),
        ("frac13", "\u{2153}", true),
        ("frac14", "\u{bc}", false),
        ("frac15", "\u{2155}", true),
        ("frac16", "\u{2159}", true),
        ("frac18", "\u{215b}", true),
        ("frac23", "\u{2154}", true),
        ("frac25", "\u{2156}", true),
        ("frac34", "\u{be}", false),
        ("frac35", "\u{2157}", true),
        ("frac38", "\u{215c}", true),
        ("frac45", "\u{2158}", true),
        ("frac56", "\u{215a}", true),
        ("frac58", "\u{215d}", true),
        ("frac78", "\u{215e}", true),
        ("frasl", "\u{2044}", true),
        ("frown", "\u{2322}", true),
        ("fscr", "\u{1d4bb}", true),
        ("gE", "\u{2267}", true),
        ("gEl", "\u{2a8c}", true),
        ("gacute", "\u{1f5}", true),
        ("gamma", "\u{3b3}", true),
        ("gammad", "\u{3dd}", true),
        ("gap", "\u{2a86}", true),
        ("gbreve", "\u{11f}", true),
        ("gcirc", "\u{11d}", true),
        ("gcy", "\u{433}", true),
        ("gdot", "\u{121}", true),
        ("ge", "\u{2265}", true),
        ("gel", "\u{22db}", true),
        ("geq", "\u{2265}", true),
        ("geqq", "\u{2267}", true),
        ("geqslant", "\u{2a7e}", true),
        ("ges", "\u{2a7e}", true),
        ("gescc", "\u{2aa9}", true),
        ("gesdot", "\u{2a80}", true),
        ("gesdoto", "\u{2a82}", true),
        ("gesdotol", "\u{2a84}", true),
        ("gesl", "\u{22db}\u{fe00}", true),
        ("gesles", "\u{2a94}", true),
        ("gfr", "\u{1d524}", true),
        ("gg", "\u{226b}", true),
        ("ggg", "\u{22d9}", true),
        ("gimel", "\u{2137}", true),
        ("gjcy", "\u{453}", true),
        ("gl", "\u{2277}", true),
        ("glE", "\u{2a92}", true),
        ("gla", "\u{2aa5}", true),
        ("glj", "\u{2aa4}", true),
        ("gnE", "\u{2269}", true),
        ("gnap", "\u{2a8a}", true),
        ("gnapprox", "\u{2a8a}", true),
        ("gne", "\u{2a88}", true),
        ("gneq", "\u{2a88}", true),
        ("gneqq", "\u{2269}", true),
        ("gnsim", "\u{22e7}", true),
        ("gopf", "\u{1d558}", true),
        ("grave", "\u{60}", true),
        ("gscr", "\u{210a}", true),
        ("gsim", "\u{2273}", true),
        ("gsime", "\u{2a8e}", true),
        ("gsiml", "\u{2a90}", true),
        ("gt", "\u{3e}", false),
        ("gtcc", "\u{2aa7}", true),
        ("gtcir", "\u{2a7a}", true),
        ("gtdot", "\u{22d7}", true),
        ("gtlPar", "\u{2995}", true),
        ("gtquest", "\u{2a7c}", true),
        ("gtrapprox", "\u{2a86}", true),
        ("gtrarr", "\u{2978}", true),
        ("gtrdot", "\u{22d7}", true),
        ("gtreqless", "\u{22db}", true),
        ("gtreqqless", "\u{2a8c}", true),
        ("gtrless", "\u{2277}", true),
        ("gtrsim", "\u{2273}", true),
        ("gvertneqq", "\u{2269}\u{fe00}", true),
        ("gvnE", "\u{2269}\u{fe00}", true),
        ("hArr", "\u{21d4}", true),
        ("hairsp", "\u{200a}", true),
        ("half", "\u{bd}", true),
        ("hamilt", "\u{210b}", true),
        ("hardcy", "\u{44a}", true),
        ("harr", "\u{2194}", true),
        ("harrcir", "\u{2948}", true),
        ("harrw", "\u{21ad}", true),
        ("hbar", "\u{210f}", true),
        ("hcirc", "\u{125}", true),
        ("hearts", "\u{2665}", true),
        ("heartsuit", "\u{2665}", true),
        ("hellip", "\u{2026}", true),
        ("hercon", "\u{22b9}", true),
        ("hfr", "\u{1d525}", true),
        ("hksearow", "\u{2925}", true),
        ("hkswarow", "\u{2926}", true),
        ("hoarr", "\u{21ff}", true),
        ("homtht", "\u{223b}", true),
        ("hookleftarrow", "\u{21a9}", true),
        ("hookrightarrow", "\u{21aa}", true),
        ("hopf", "\u{1d559}", true),
        ("horbar", "\u{2015}", true),
        ("hscr", "\u{1d4bd}", true),
        ("hslash", "\u{210f}", true),
        ("hstrok", "\u{127}", true),
        ("hybull", "\u{2043}", true),
        ("hyphen", "\u{2010}", true),
        ("iacute", "\u{ed}", false),
        ("ic", "\u{2063}", true),
        ("icirc", "\u{ee}", false),
        ("icy", "\u{438}", true),
        ("iecy", "\u{435}", true),
        ("iexcl", "\u{a1}", false),
        ("iff", "\u{21d4}", true),
        ("ifr", "\u{1d526}", true),
        ("igrave", "\u{ec}", false),
        ("ii", "\u{2148}", true),
        ("iiiint", "\u{2a0c}", true),
        ("iiint", "\u{222d}", true),
        ("iinfin", "\u{29dc}", true),
        ("iiota", "\u{2129}", true),
        ("ijlig", "\u{133}", true),
        ("imacr", "\u{12b}", true),
        ("image", "\u{2111}", true),
        ("imagline", "\u{2110}", true),
        ("imagpart", "\u{2111}", true),
        ("imath", "\u{131}", true),
        ("imof", "\u{22b7}", true),
        ("imped", "\u{1b5}", true),
        ("in", "\u{2208}", true),
        ("incare", "\u{2105}", true),
        ("infin", "\u{221e}", true),
        ("infintie", "\u{29dd}", true),
        ("inodot", "\u{131}", true),
        ("int", "\u{222b}", true),
        ("intcal", "\u{22ba}", true),
        ("integers", "\u{2124}", true),
        ("intercal", "\u{22ba}", true),
        ("intlarhk", "\u{2a17}", true),
        ("intprod", "\u{2a3c}", true),
        ("iocy", "\u{451}", true),
        ("iogon", "\u{12f}", true),
        ("iopf", "\u{1d55a}", true),
        ("iota", "\u{3b9}", true),
        ("iprod", "\u{2a3c}", true),
        ("iquest", "\u{bf}", false),
        ("iscr", "\u{1d4be}", true),
        ("isin", "\u{2208}", true),
        ("isinE", "\u{22f9}", true),
        ("isindot", "\u{22f5}", true),
        ("isins", "\u{22f4}", true),
        ("isinsv", "\u{22f3}", true),
        ("isinv", "\u{2208}", true),
        ("it", "\u{2062}", true),
        ("itilde", "\u{129}", true),
        ("iukcy", "\u{456}", true),
        ("iuml", "\u{ef}", false),
        ("jcirc", "\u{135}", true),
        ("jcy", "\u{439}", true),
        ("jfr", "\u{1d527}", true),
        ("jmath", "\u{237}", true),
        ("jopf", "\u{1d55b}", true),
        ("jscr", "\u{1d4bf}", true),
        ("jsercy", "\u{458}", true),
        ("jukcy", "\u{454}", true),
        ("kappa", "\u{3ba}", true),
        ("kappav", "\u{3f0}", true),
        ("kcedil", "\u{137}", true),
        ("kcy", "\u{43a}", true),
        ("kfr", "\u{1d528}", true),
        ("kgreen", "\u{138}", true),
        ("khcy", "\u{445}", true),
        ("kjcy", "\u{45c}", true),
        ("kopf", "\u{1d55c}", true),
        ("kscr", "\u{1d4c0}", true),
        ("lAarr", "\u{21da}", true),
        ("lArr", "\u{21d0}", true),
        ("lAtail", "\u{291b}", true),
        ("lBarr", "\u{290e}", true),
        ("lE", "\u{2266}", true),
        ("lEg", "\u{2a8b}", true),
        ("lHar", "\u{2962}", true),
        ("lacute", "\u{13a}", true),
        ("laemptyv", "\u{29b4}", true),
        ("lagran", "\u{2112}", true),
        ("lambda", "\u{3bb}", true),
        ("lang", "\u{27e8}", true),
        ("langd", "\u{2991}", true),
        ("langle", "\u{27e8}", true),
        ("lap", "\u{2a85}", true),
        ("laquo", "\u{ab}", false),
        ("larr", "\u{2190}", true),
        ("larrb", "\u{21e4}", true),
        ("larrbfs", "\u{291f}", true),
        ("larrfs", "\u{291d}", true),
        ("larrhk", "\u{21a9}", true),
        ("larrlp", "\u{21ab}", true),
        ("larrpl", "\u{2939}", true),
        ("larrsim", "\u{2973}", true),
        ("larrtl", "\u{21a2}", true),
        ("lat", "\u{2aab}", true),
        ("latail", "\u{2919}", true),
        ("late", "\u{2aad}", true),
        ("lates", "\u{2aad}\u{fe00}", true),
        ("lbarr", "\u{290c}", true),
        ("lbbrk", "\u{2772}", true),
        ("lbrace", "\u{7b}", true),
        ("lbrack", "\u{5b}", true),
        ("lbrke", "\u{298b}", true),
        ("lbrksld", "\u{298f}", true),
        ("lbrkslu", "\u{298d}", true),
        ("lcaron", "\u{13e}", true),
        ("lcedil", "\u{13c}", true),
        ("lceil", "\u{2308}", true),
        ("lcub", "\u{7b}", true),
        ("lcy", "\u{43b}", true),
        ("ldca", "\u{2936}", true),
        ("ldquo", "\u{201c}", true),
        ("ldquor", "\u{201e}", true),
        ("ldrdhar", "\u{2967}", true),
        ("ldrushar", "\u{294b}", true),
        ("ldsh", "\u{21b2}", true),
        ("le", "\u{2264}", true),
        ("leftarrow", "\u{2190}", true),
        ("leftarrowtail", "\u{21a2}", true),
        ("leftharpoondown", "\u{21bd}", true),
        ("leftharpoonup", "\u{21bc}", true),
        ("leftleftarrows", "\u{21c7}", true),
        ("leftrightarrow", "\u{2194}", true),
        ("leftrightarrows", "\u{21c6}", true),
        ("leftrightharpoons", "\u{21cb}", true),
        ("leftrightsquigarrow", "\u{21ad}", true),
        ("leftthreetimes", "\u{22cb}", true),
        ("leg", "\u{22da}", true),
        ("leq", "\u{2264}", true),
        ("leqq", "\u{2266}", true),
        ("leqslant", "\u{2a7d}", true),
        ("les", "\u{2a7d}", true),
        ("lescc", "\u{2aa8}", true),
        ("lesdot", "\u{2a7f}", true),
        ("lesdoto", "\u{2a81}", true),
        ("lesdotor", "\u{2a83}", true),
        ("lesg", "\u{22da}\u{fe00}", true),
        ("lesges", "\u{2a93}", true),
        ("lessapprox", "\u{2a85}", true),
        ("lessdot", "\u{22d6}", true),
        ("lesseqgtr", "\u{22da}", true),
        ("lesseqqgtr", "\u{2a8b}", true),
        ("lessgtr", "\u{2276}", true),
        ("lesssim", "\u{2272}", true),
        ("lfisht", "\u{297c}", true),
        ("lfloor", "\u{230a}", true),
        ("lfr", "\u{1d529}", true),
        ("lg", "\u{2276}", true),
        ("lgE", "\u{2a91}", true),
        ("lhard", "\u{21bd}", true),
        ("lharu", "\u{21bc}", true),
        ("lharul", "\u{296a}", true),
        ("lhblk", "\u{2584}", true),
        ("ljcy", "\u{459}", true),
        ("ll", "\u{226a}", true),
        ("llarr", "\u{21c7}", true),
        ("llcorner", "\u{231e}", true),
        ("llhard", "\u{296b}", true),
        ("lltri", "\u{25fa}", true),
        ("lmidot", "\u{140}", true),
        ("lmoust", "\u{23b0}", true),
        ("lmoustache", "\u{23b0}", true),
        ("lnE", "\u{2268}", true),
        ("lnap", "\u{2a89}", true),
        ("lnapprox", "\u{2a89}", true),
        ("lne", "\u{2a87}", true),
        ("lneq", "\u{2a87}", true),
        ("lneqq", "\u{2268}", true),
        ("lnsim", "\u{22e6}", true),
        ("loang", "\u{27ec}", true),
        ("loarr", "\u{21fd}", true),
        ("lobrk", "\u{27e6}", true),
        ("longleftarrow", "\u{27f5}", true),
        ("longleftrightarrow", "\u{27f7}", true),
        ("longmapsto", "\u{27fc}", true),
        ("longrightarrow", "\u{27f6}", true),
        ("looparrowleft", "\u{21ab}", true),
        ("looparrowright", "\u{21ac}", true),
        ("lopar", "\u{2985}", true),
        ("lopf", "\u{1d55d}", true),
        ("loplus", "\u{2a2d}", true),
        ("lotimes", "\u{2a34}", true),
        ("lowast", "\u{2217}", true),
        ("lowbar", "\u{5f}", true),
        ("loz", "\u{25ca}", true),
        ("lozenge", "\u{25ca}", true),
        ("lozf", "\u{29eb}", true),
        ("lpar", "\u{28}", true),
        ("lparlt", "\u{2993}", true),
        ("lrarr", "\u{21c6}", true),
        ("lrcorner", "\u{231f}", true),
        ("lrhar", "\u{21cb}", true),
        ("lrhard", "\u{296d}", true),
        ("lrm", "\u{200e}", true),
        ("lrtri", "\u{22bf}", true),
        ("lsaquo", "\u{2039}", true),
        ("lscr", "\u{1d4c1}", true),
        ("lsh", "\u{21b0}", true),
        ("lsim", "\u{2272}", true),
        ("lsime", "\u{2a8d}", true),
        ("lsimg", "\u{2a8f}", true),
        ("lsqb", "\u{5b}", true),
        ("lsquo", "\u{2018}", true),
        ("lsquor", "\u{201a}", true),
        ("lstrok", "\u{142}", true),
        ("lt", "\u{3c}", false),
        ("ltcc", "\u{2aa6}", true),
        ("ltcir", "\u{2a79}", true),
        ("ltdot", "\u{22d6}", true),
        ("lthree", "\u{22cb}", true),
        ("ltimes", "\u{22c9}", true),
        ("ltlarr", "\u{2976}", true),
        ("ltquest", "\u{2a7b}", true),
        ("ltrPar", "\u{2996}", true),
        ("ltri", "\u{25c3}", true),
        ("ltrie", "\u{22b4}", true),
        ("ltrif", "\u{25c2}", true),
        ("lurdshar", "\u{294a}", true),
        ("luruhar", "\u{2966}", true),
        ("lvertneqq", "\u{2268}\u{fe00}", true),
        ("lvnE", "\u{2268}\u{fe00}", true),
        ("mDDot", "\u{223a}", true),
        ("macr", "\u{af}", false),
        ("male", "\u{2642}", true),
        ("malt", "\u{2720}", true),
        ("maltese", "\u{2720}", true),
        ("map", "\u{21a6}", true),
        ("mapsto", "\u{21a6}", true),
        ("mapstodown", "\u{21a7}", true),
        ("mapstoleft", "\u{21a4}", true),
        ("mapstoup", "\u{21a5}", true),
        ("marker", "\u{25ae}", true),
        ("mcomma", "\u{2a29}", true),
        ("mcy", "\u{43c}", true),
        ("mdash", "\u{2014}", true),
        ("measuredangle", "\u{2221}", true),
        ("mfr", "\u{1d52a}", true),
        ("mho", "\u{2127}", true),
        ("micro", "\u{b5}", false),
        ("mid", "\u{2223}", true),
        ("midast", "\u{2a}", true),
        ("midcir", "\u{2af0}", true),
        ("middot", "\u{b7}", false),
        ("minus", "\u{2212}", true),
        ("minusb", "\u{229f}", true),
        ("minusd", "\u{2238}", true),
        ("minusdu", "\u{2a2a}", true),
        ("mlcp", "\u{2adb}", true),
        ("mldr", "\u{2026}", true),
        ("mnplus", "\u{2213}", true),
        ("models", "\u{22a7}", true),
        ("mopf", "\u{1d55e}", true),
        ("mp", "\u{2213}", true),
        ("mscr", "\u{1d4c2}", true),
        ("mstpos", "\u{223e}", true),
        ("mu", "\u{3bc}", true),
        ("multimap", "\u{22b8}", true),
        ("mumap", "\u{22b8}", true),
        ("nGg", "\u{22d9}\u{338}", true),
        ("nGt", "\u{226b}\u{20d2}", true),
        ("nGtv", "\u{226b}\u{338}", true),
        ("nLeftarrow", "\u{21cd}", true),
        ("nLeftrightarrow", "\u{21ce}", true),
        ("nLl", "\u{22d8}\u{338}", true),
        ("nLt", "\u{226a}\u{20d2}", true),
        ("nLtv", "\u{226a}\u{338}", true),
        ("nRightarrow", "\u{21cf}", true),
        ("nVDash", "\u{22af}", true),
        ("nVdash", "\u{22ae}", true),
        ("nabla", "\u{2207}", true),
        ("nacute", "\u{144}", true),
        ("nang", "\u{2220}\u{20d2}", true),
        ("nap", "\u{2249}", true),
        ("napE", "\u{2a70}\u{338}", true),
        ("napid", "\u{224b}\u{338}", true),
        ("napos", "\u{149}", true),
        ("napprox", "\u{2249}", true),
        ("natur", "\u{266e}", true),
        ("natural", "\u{266e}", true),
        ("naturals", "\u{2115}", true),
        ("nbsp", "\u{a0}", false),
        ("nbump", "\u{224e}\u{338}", true),
        ("nbumpe", "\u{224f}\u{338}", true),
        ("ncap", "\u{2a43}", true),
        ("ncaron", "\u{148}", true),
        ("ncedil", "\u{146}", true),
        ("ncong", "\u{2247}", true),
        ("ncongdot", "\u{2a6d}\u{338}", true),
        ("ncup", "\u{2a42}", true),
        ("ncy", "\u{43d}", true),
        ("ndash", "\u{2013}", true),
        ("ne", "\u{2260}", true),
        ("neArr", "\u{21d7}", true),
        ("nearhk", "\u{2924}", true),
        ("nearr", "\u{2197}", true),
        ("nearrow", "\u{2197}", true),
        ("nedot", "\u{2250}\u{338}", true),
        ("nequiv", "\u{2262}", true),
        ("nesear", "\u{2928}", true),
        ("nesim", "\u{2242}\u{338}", true),
        ("nexist", "\u{2204}", true),
        ("nexists", "\u{2204}", true),
        ("nfr", "\u{1d52b}", true),
        ("ngE", "\u{2267}\u{338}", true),
        ("nge", "\u{2271}", true),
        ("ngeq", "\u{2271}", true),
        ("ngeqq", "\u{2267}\u{338}", true),
        ("ngeqslant", "\u{2a7e}\u{338}", true),
        ("nges", "\u{2a7e}\u{338}", true),
        ("ngsim", "\u{2275}", true),
        ("ngt", "\u{226f}", true),
        ("ngtr", "\u{226f}", true),
        ("nhArr", "\u{21ce}", true),
        ("nharr", "\u{21ae}", true),
        ("nhpar", "\u{2af2}", true),
        ("ni", "\u{220b}", true),
        ("nis", "\u{22fc}", true),
        ("nisd", "\u{22fa}", true),
        ("niv", "\u{220b}", true),
        ("njcy", "\u{45a}", true),
        ("nlArr", "\u{21cd}", true),
        ("nlE", "\u{2266}\u{338}", true),
        ("nlarr", "\u{219a}", true),
        ("nldr", "\u{2025}", true),
        ("nle", "\u{2270}", true),
        ("nleftarrow", "\u{219a}", true),
        ("nleftrightarrow", "\u{21ae}", true),
        ("nleq", "\u{2270}", true),
        ("nleqq", "\u{2266}\u{338}", true),
        ("nleqslant", "\u{2a7d}\u{338}", true),
        ("nles", "\u{2a7d}\u{338}", true),
        ("nless", "\u{226e}", true),
        ("nlsim", "\u{2274}", true),
        ("nlt", "\u{226e}", true),
        ("nltri", "\u{22ea}", true),
        ("nltrie", "\u{22ec}", true),
        ("nmid", "\u{2224}", true),
        ("nopf", "\u{1d55f}", true),
        ("not", "\u{ac}", false),
        ("notin", "\u{2209}", true),
        ("notinE", "\u{22f9}\u{338}", true),
        ("notindot", "\u{22f5}\u{338}", true),
        ("notinva", "\u{2209}", true),
        ("notinvb", "\u{22f7}", true),
        ("notinvc", "\u{22f6}", true),
        ("notni", "\u{220c}", true),
        ("notniva", "\u{220c}", true),
        ("notnivb", "\u{22fe}", true),
        ("notnivc", "\u{22fd}", true),
        ("npar", "\u{2226}", true),
        ("nparallel", "\u{2226}", true),
        ("nparsl", "\u{2afd}\u{20e5}", true),
        ("npart", "\u{2202}\u{338}", true),
        ("npolint", "\u{2a14}", true),
        ("npr", "\u{2280}", true),
        ("nprcue", "\u{22e0}", true),
        ("npre", "\u{2aaf}\u{338}", true),
        ("nprec", "\u{2280}", true),
        ("npreceq", "\u{2aaf}\u{338}", true),
        ("nrArr", "\u{21cf}", true),
        ("nrarr", "\u{219b}", true),
        ("nrarrc", "\u{2933}\u{338}", true),
        ("nrarrw", "\u{219d}\u{338}", true),
        ("nrightarrow", "\u{219b}", true),
        ("nrtri", "\u{22eb}", true),
        ("nrtrie", "\u{22ed}", true),
        ("nsc", "\u{2281}", true),
        ("nsccue", "\u{22e1}", true),
        ("nsce", "\u{2ab0}\u{338}", true),
        ("nscr", "\u{1d4c3}", true),
        ("nshortmid", "\u{2224}", true),
        ("nshortparallel", "\u{2226}", true),
        ("nsim", "\u{2241}", true),
        ("nsime", "\u{2244}", true),
        ("nsimeq", "\u{2244}", true),
        ("nsmid", "\u{2224}", true),
        ("nspar", "\u{2226}", true),
        ("nsqsube", "\u{22e2}", true),
        ("nsqsupe", "\u{22e3}", true),
        ("nsub", "\u{2284}", true),
        ("nsubE", "\u{2ac5}\u{338}", true),
        ("nsube", "\u{2288}", true),
        ("nsubset", "\u{2282}\u{20d2}", true),
        ("nsubseteq", "\u{2288}", true),
        ("nsubseteqq", "\u{2ac5}\u{338}", true),
        ("nsucc", "\u{2281}", true),
        ("nsucceq", "\u{2ab0}\u{338}", true),
        ("nsup", "\u{2285}", true),
        ("nsupE", "\u{2ac6}\u{338}", true),
        ("nsupe", "\u{2289}", true),
        ("nsupset", "\u{2283}\u{20d2}", true),
        ("nsupseteq", "\u{2289}", true),
        ("nsupseteqq", "\u{2ac6}\u{338}", true),
        ("ntgl", "\u{2279}", true),
        ("ntilde", "\u{f1}", false),
        ("ntlg", "\u{2278}", true),
        ("ntriangleleft", "\u{22ea}", true),
        ("ntrianglelefteq", "\u{22ec}", true),
        ("ntriangleright", "\u{22eb}", true),
        ("ntrianglerighteq", "\u{22ed}", true),
        ("nu", "\u{3bd}", true),
        ("num", "\u{23}", true),
        ("numero", "\u{2116}", true),
        ("numsp", "\u{2007}", true),
        ("nvDash", "\u{22ad}", true),
        ("nvHarr", "\u{2904}", true),
        ("nvap", "\u{224d}\u{20d2}", true),
        ("nvdash", "\u{22ac}", true),
        ("nvge", "\u{2265}\u{20d2}", true),
        ("nvgt", "\u{3e}\u{20d2}", true),
        ("nvinfin", "\u{29de}", true),
        ("nvlArr", "\u{2902}", true),
        ("nvle", "\u{2264}\u{20d2}", true),
        ("nvlt", "\u{3c}\u{20d2}", true),
        ("nvltrie", "\u{22b4}\u{20d2}", true),
        ("nvrArr", "\u{2903}", true),
        ("nvrtrie", "\u{22b5}\u{20d2}", true),
        ("nvsim", "\u{223c}\u{20d2}", true),
        ("nwArr", "\u{21d6}", true),
        ("nwarhk", "\u{2923}", true),
        ("nwarr", "\u{2196}", true),
        ("nwarrow", "\u{2196}", true),
        ("nwnear", "\u{2927}", true),
        ("oS", "\u{24c8}", true),
        ("oacute", "\u{f3}", false),
        ("oast", "\u{229b}", true),
        ("ocir", "\u{229a}", true),
        ("ocirc", "\u{f4}", false),
        ("ocy", "\u{43e}", true),
        ("odash", "\u{229d}", true),
        ("odblac", "\u{151}", true),
        ("odiv", "\u{2a38}", true),
        ("odot", "\u{2299}", true),
        ("odsold", "\u{29bc}", true),
        ("oelig", "\u{153}", true),
        ("ofcir", "\u{29bf}", true),
        ("ofr", "\u{1d52c}", true),
        ("ogon", "\u{2db}", true),
        ("ograve", "\u{f2}", false),
        ("ogt", "\u{29c1}", true),
        ("ohbar", "\u{29b5}", true),
        ("ohm", "\u{3a9}", true),
        ("oint", "\u{222e}", true),
        ("olarr", "\u{21ba}", true),
        ("olcir", "\u{29be}", true),
        ("olcross", "\u{29bb}", true),
        ("oline", "\u{203e}", true),
        ("olt", "\u{29c0}", true),
        ("omacr", "\u{14d}", true),
        ("omega", "\u{3c9}", true),
        ("omicron", "\u{3bf}", true),
        ("omid", "\u{29b6}", true),
        ("ominus", "\u{2296}", true),
        ("oopf", "\u{1d560}", true),
        ("opar", "\u{29b7}", true),
        ("operp", "\u{29b9}", true),
        ("oplus", "\u{2295}", true),
        ("or", "\u{2228}", true),
        ("orarr", "\u{21bb}", true),
        ("ord", "\u{2a5d}", true),
        ("order", "\u{2134}", true),
        ("orderof", "\u{2134}", true),
        ("ordf", "\u{aa}", false),
        ("ordm", "\u{ba}", false),
        ("origof", "\u{22b6}", true),
        ("oror", "\u{2a56}", true),
        ("orslope", "\u{2a57}", true),
        ("orv", "\u{2a5b}", true),
        ("oscr", "\u{2134}", true),
        ("oslash", "\u{f8}", false),
        ("osol", "\u{2298}", true),
        ("otilde", "\u{f5}", false),
        ("otimes", "\u{2297}", true),
        ("otimesas", "\u{2a36}", true),
        ("ouml", "\u{f6}", false),
        ("ovbar", "\u{233d}", true),
        ("par", "\u{2225}", true),
        ("para", "\u{b6}", false),
        ("parallel", "\u{2225}", true),
        ("parsim", "\u{2af3}", true),
        ("parsl", "\u{2afd}", true),
        ("part", "\u{2202}", true),
        ("pcy", "\u{43f}", true),
        ("percnt", "\u{25}", true),
        ("period", "\u{2e}", true),
        ("permil", "\u{2030}", true),
        ("perp", "\u{22a5}", true),
        ("pertenk", "\u{2031}", true),
        ("pfr", "\u{1d52d}", true),
        ("phi", "\u{3c6}", true),
        ("phiv", "\u{3d5}", true),
        ("phmmat", "\u{2133}", true),
        ("phone", "\u{260e}", true),
        ("pi", "\u{3c0}", true),
        ("pitchfork", "\u{22d4}", true),
        ("piv", "\u{3d6}", true),
        ("planck", "\u{210f}", true),
        ("planckh", "\u{210e}", true),
        ("plankv", "\u{210f}", true),
        ("plus", "\u{2b}", true),
        ("plusacir", "\u{2a23}", true),
        ("plusb", "\u{229e}", true),
        ("pluscir", "\u{2a22}", true),
        ("plusdo", "\u{2214}", true),
        ("plusdu", "\u{2a25}", true),
        ("pluse", "\u{2a72}", true),
        ("plusmn", "\u{b1}", false),
        ("plussim", "\u{2a26}", true),
        ("plustwo", "\u{2a27}", true),
        ("pm", "\u{b1}", true),
        ("pointint", "\u{2a15}", true),
        ("popf", "\u{1d561}", true),
        ("pound", "\u{a3}", false),
        ("pr", "\u{227a}", true),
        ("prE", "\u{2ab3}", true),
        ("prap", "\u{2ab7}", true),
        ("prcue", "\u{227c}", true),
        ("pre", "\u{2aaf}", true),
        ("prec", "\u{227a}", true),
        ("precapprox", "\u{2ab7}", true),
        ("preccurlyeq", "\u{227c}", true),
        ("preceq", "\u{2aaf}", true),
        ("precnapprox", "\u{2ab9}", true),
        ("precneqq", "\u{2ab5}", true),
        ("precnsim", "\u{22e8}", true),
        ("precsim", "\u{227e}", true),
        ("prime", "\u{2032}", true),
        ("primes", "\u{2119}", true),
        ("prnE", "\u{2ab5}", true),
        ("prnap", "\u{2ab9}", true),
        ("prnsim", "\u{22e8}", true),
        ("prod", "\u{220f}", true),
        ("profalar", "\u{232e}", true),
        ("profline", "\u{2312}", true),
        ("profsurf", "\u{2313}", true),
        ("prop", "\u{221d}", true),
        ("propto", "\u{221d}", true),
        ("prsim", "\u{227e}", true),
        ("prurel", "\u{22b0}", true),
        ("pscr", "\u{1d4c5}", true),
        ("psi", "\u{3c8}", true),
        ("puncsp", "\u{2008}", true),
        ("qfr", "\u{1d52e}", true),
        ("qint", "\u{2a0c}", true),
        ("qopf", "\u{1d562}", true),
        ("qprime", "\u{2057}", true),
        ("qscr", "\u{1d4c6}", true),
        ("quaternions", "\u{210d}", true),
        ("quatint", "\u{2a16}", true),
        ("quest", "\u{3f}", true),
        ("questeq", "\u{225f}", true),
        ("quot", "\u{22}", false),
        ("rAarr", "\u{21db}", true),
        ("rArr", "\u{21d2}", true),
        ("rAtail", "\u{291c}", true),
        ("rBarr", "\u{290f}", true),
        ("rHar", "\u{2964}", true),
        ("race", "\u{223d}\u{331}", true),
        ("racute", "\u{155}", true),
        ("radic", "\u{221a}", true),
        ("raemptyv", "\u{29b3}", true),
        ("rang", "\u{27e9}", true),
        ("rangd", "\u{2992}", true),
        ("range", "\u{29a5}", true),
        ("rangle", "\u{27e9}", true),
        ("raquo", "\u{bb}", false),
        ("rarr", "\u{2192}", true),
        ("rarrap", "\u{2975}", true),
        ("rarrb", "\u{21e5}", true),
        ("rarrbfs", "\u{2920}", true),
        ("rarrc", "\u{2933}", true),
        ("rarrfs", "\u{291e}", true),
        ("rarrhk", "\u{21aa}", true),
        ("rarrlp", "\u{21ac}", true),
        ("rarrpl", "\u{2945}", true),
        ("rarrsim", "\u{2974}", true),
        ("rarrtl", "\u{21a3}", true),
        ("rarrw", "\u{219d}", true),
        ("ratail", "\u{291a}", true),
        ("ratio", "\u{2236}", true),
        ("rationals", "\u{211a}", true),
        ("rbarr", "\u{290d}", true),
        ("rbbrk", "\u{2773}", true),
        ("rbrace", "\u{7d}", true),
        ("rbrack", "\u{5d}", true),
        ("rbrke", "\u{298c}", true),
        ("rbrksld", "\u{298e}", true),
        ("rbrkslu", "\u{2990}", true),
        ("rcaron", "\u{159}", true),
        ("rcedil", "\u{157}", true),
        ("rceil", "\u{2309}", true),
        ("rcub", "\u{7d}", true),
        ("rcy", "\u{440}", true),
        ("rdca", "\u{2937}", true),
        ("rdldhar", "\u{2969}", true),
        ("rdquo", "\u{201d}", true),
        ("rdquor", "\u{201d}", true),
        ("rdsh", "\u{21b3}", true),
        ("real", "\u{211c}", true),
        ("realine", "\u{211b}", true),
        ("realpart", "\u{211c}", true),
        ("reals", "\u{211d}", true),
        ("rect", "\u{25ad}", true),
        ("reg", "\u{ae}", false),
        ("rfisht", "\u{297d}", true),
        ("rfloor", "\u{230b}", true),
        ("rfr", "\u{1d52f}", true),
        ("rhard", "\u{21c1}", true),
        ("rharu", "\u{21c0}", true),
        ("rharul", "\u{296c}", true),
        ("rho", "\u{3c1}", true),
        ("rhov", "\u{3f1}", true),
        ("rightarrow", "\u{2192}", true),
        ("rightarrowtail", "\u{21a3}", true),
        ("rightharpoondown", "\u{21c1}", true),
        ("rightharpoonup", "\u{21c0}", true),
        ("rightleftarrows", "\u{21c4}", true),
        ("rightleftharpoons", "\u{21cc}", true),
        ("rightrightarrows", "\u{21c9}", true),
        ("rightsquigarrow", "\u{219d}", true),
        ("rightthreetimes", "\u{22cc}", true),
        ("ring", "\u{2da}", true),
        ("risingdotseq", "\u{2253}", true),
        ("rlarr", "\u{21c4}", true),
        ("rlhar", "\u{21cc}", true),
        ("rlm", "\u{200f}", true),
        ("rmoust", "\u{23b1}", true),
        ("rmoustache", "\u{23b1}", true),
        ("rnmid", "\u{2aee}", true),
        ("roang", "\u{27ed}", true),
        ("roarr", "\u{21fe}", true),
        ("robrk", "\u{27e7}", true),
        ("ropar", "\u{2986}", true),
        ("ropf", "\u{1d563}", true),
        ("roplus", "\u{2a2e}", true),
        ("rotimes", "\u{2a35}", true),
        ("rpar", "\u{29}", true),
        ("rpargt", "\u{2994}", true),
        ("rppolint", "\u{2a12}", true),
        ("rrarr", "\u{21c9}", true),
        ("rsaquo", "\u{203a}", true),
        ("rscr", "\u{1d4c7}", true),
        ("rsh", "\u{21b1}", true),
        ("rsqb", "\u{5d}", true),
        ("rsquo", "\u{2019}", true),
        ("rsquor", "\u{2019}", true),
        ("rthree", "\u{22cc}", true),
        ("rtimes", "\u{22ca}", true),
        ("rtri", "\u{25b9}", true),
        ("rtrie", "\u{22b5}", true),
        ("rtrif", "\u{25b8}", true),
        ("rtriltri", "\u{29ce}", true),
        ("ruluhar", "\u{2968}", true),
        ("rx", "\u{211e}", true),
        ("sacute", "\u{15b}", true),
        ("sbquo", "\u{201a}", true),
        ("sc", "\u{227b}", true),
        ("scE", "\u{2ab4}", true),
        ("scap", "\u{2ab8}", true),
        ("scaron", "\u{161}", true),
        ("sccue", "\u{227d}", true),
        ("sce", "\u{2ab0}", true),
        ("scedil", "\u{15f}", true),
        ("scirc", "\u{15d}", true),
        ("scnE", "\u{2ab6}", true),
        ("scnap", "\u{2aba}", true),
        ("scnsim", "\u{22e9}", true),
        ("scpolint", "\u{2a13}", true),
        ("scsim", "\u{227f}", true),
        ("scy", "\u{441}", true),
        ("sdot", "\u{22c5}", true),
        ("sdotb", "\u{22a1}", true),
        ("sdote", "\u{2a66}", true),
        ("seArr", "\u{21d8}", true),
        ("searhk", "\u{2925}", true),
        ("searr", "\u{2198}", true),
        ("searrow", "\u{2198}", true),
        ("sect", "\u{a7}", false),
        ("semi", "\u{3b}", true),
        ("seswar", "\u{2929}", true),
        ("setminus", "\u{2216}", true),
        ("setmn", "\u{2216}", true),
        ("sext", "\u{2736}", true),
        ("sfr", "\u{1d530}", true),
        ("sfrown", "\u{2322}", true),
        ("sharp", "\u{266f}", true),
        ("shchcy", "\u{449}", true),
        ("shcy", "\u{448}", true),
        ("shortmid", "\u{2223}", true),
        ("shortparallel", "\u{2225}", true),
        ("shy", "\u{ad}", false),
        ("sigma", "\u{3c3}", true),
        ("sigmaf", "\u{3c2}", true),
        ("sigmav", "\u{3c2}", true),
        ("sim", "\u{223c}", true),
        ("simdot", "\u{2a6a}", true),
        ("sime", "\u{2243}", true),
        ("simeq", "\u{2243}", true),
        ("simg", "\u{2a9e}", true),
        ("simgE", "\u{2aa0}", true),
        ("siml", "\u{2a9d}", true),
        ("simlE", "\u{2a9f}", true),
        ("simne", "\u{2246}", true),
        ("simplus", "\u{2a24}", true),
        ("simrarr", "\u{2972}", true),
        ("slarr", "\u{2190}", true),
        ("smallsetminus", "\u{2216}", true),
        ("smashp", "\u{2a33}", true),
        ("smeparsl", "\u{29e4}", true),
        ("smid", "\u{2223}", true),
        ("smile", "\u{2323}", true),
        ("smt", "\u{2aaa}", true),
        ("smte", "\u{2aac}", true),
        ("smtes", "\u{2aac}\u{fe00}", true),
        ("softcy", "\u{44c}", true),
        ("sol", "\u{2f}", true),
        ("solb", "\u{29c4}", true),
        ("solbar", "\u{233f}", true),
        ("sopf", "\u{1d564}", true),
        ("spades", "\u{2660}", true),
        ("spadesuit", "\u{2660}", true),
        ("spar", "\u{2225}", true),
        ("sqcap", "\u{2293}", true),
        ("sqcaps", "\u{2293}\u{fe00}", true),
        ("sqcup", "\u{2294}", true),
        ("sqcups", "\u{2294}\u{fe00}", true),
        ("sqsub", "\u{228f}", true),
        ("sqsube", "\u{2291}", true),
        ("sqsubset", "\u{228f}", true),
        ("sqsubseteq", "\u{2291}", true),
        ("sqsup", "\u{2290}", true),
        ("sqsupe", "\u{2292}", true),
        ("sqsupset", "\u{2290}", true),
        ("sqsupseteq", "\u{2292}", true),
        ("squ", "\u{25a1}", true),
        ("square", "\u{25a1}", true),
        ("squarf", "\u{25aa}", true),
        ("squf", "\u{25aa}", true),
        ("srarr", "\u{2192}", true),
        ("sscr", "\u{1d4c8}", true),
        ("ssetmn", "\u{2216}", true),
        ("ssmile", "\u{2323}", true),
        ("sstarf", "\u{22c6}", true),
        ("star", "\u{2606}", true),
        ("starf", "\u{2605}", true),
        ("straightepsilon", "\u{3f5}", true),
        ("straightphi", "\u{3d5}", true),
        ("strns", "\u{af}", true),
        ("sub", "\u{2282}", true),
        ("subE", "\u{2ac5}", true),
        ("subdot", "\u{2abd}", true),
        ("sube", "\u{2286}", true),
        ("subedot", "\u{2ac3}", true),
        ("submult", "\u{2ac1}", true),
        ("subnE", "\u{2acb}", true),
        ("subne", "\u{228a}", true),
        ("subplus", "\u{2abf}", true),
        ("subrarr", "\u{2979}", true),
        ("subset", "\u{2282}", true),
        ("subseteq", "\u{2286}", true),
        ("subseteqq", "\u{2ac5}", true),
        ("subsetneq", "\u{228a}", true),
        ("subsetneqq", "\u{2acb}", true),
        ("subsim", "\u{2ac7}", true),
        ("subsub", "\u{2ad5}", true),
        ("subsup", "\u{2ad3}", true),
        ("succ", "\u{227b}", true),
        ("succapprox", "\u{2ab8}", true),
        ("succcurlyeq", "\u{227d}", true),
        ("succeq", "\u{2ab0}", true),
        ("succnapprox", "\u{2aba}", true),
        ("succneqq", "\u{2ab6}", true),
        ("succnsim", "\u{22e9}", true),
        ("succsim", "\u{227f}", true),
        ("sum", "\u{2211}", true),
        ("sung", "\u{266a}", true),
        ("sup", "\u{2283}", true),
        ("sup1", "\u{b9}", false),
        ("sup2", "\u{b2}", false),
        ("sup3", "\u{b3}", false),
        ("supE", "\u{2ac6}", true),
        ("supdot", "\u{2abe}", true),
        ("supdsub", "\u{2ad8}", true),
        ("supe", "\u{2287}", true),
        ("supedot", "\u{2ac4}", true),
        ("suphsol", "\u{27c9}", true),
        ("suphsub", "\u{2ad7}", true),
        ("suplarr", "\u{297b}", true),
        ("supmult", "\u{2ac2}", true),
        ("supnE", "\u{2acc}", true),
        ("supne", "\u{228b}", true),
        ("supplus", "\u{2ac0}", true),
        ("supset", "\u{2283}", true),
        ("supseteq", "\u{2287}", true),
        ("supseteqq", "\u{2ac6}", true),
        ("supsetneq", "\u{228b}", true),
        ("supsetneqq", "\u{2acc}", true),
        ("supsim", "\u{2ac8}", true),
        ("supsub", "\u{2ad4}", true),
        ("supsup", "\u{2ad6}", true),
        ("swArr", "\u{21d9}", true),
        ("swarhk", "\u{2926}", true),
        ("swarr", "\u{2199}", true),
        ("swarrow", "\u{2199}", true),
        ("swnwar", "\u{292a}", true),
        ("szlig", "\u{df}", false),
        ("target", "\u{2316}", true),
        ("tau", "\u{3c4}", true),
        ("tbrk", "\u{23b4}", true),
        ("tcaron", "\u{165}", true),
        ("tcedil", "\u{163}", true),
        ("tcy", "\u{442}", true),
        ("tdot", "\u{20db}", true),
        ("telrec", "\u{2315}", true),
        ("tfr", "\u{1d531}", true),
        ("there4", "\u{2234}", true),
        ("therefore", "\u{2234}", true),
        ("theta", "\u{3b8}", true),
        ("thetasym", "\u{3d1}", true),
        ("thetav", "\u{3d1}", true),
        ("thickapprox", "\u{2248}", true),
        ("thicksim", "\u{223c}", true),
        ("thinsp", "\u{2009}", true),
        ("thkap", "\u{2248}", true),
        ("thksim", "\u{223c}", true),
        ("thorn", "\u{fe}", false),
        ("tilde", "\u{2dc}", true),
        ("times", "\u{d7}", false),
        ("timesb", "\u{22a0}", true),
        ("timesbar", "\u{2a31}", true),
        ("timesd", "\u{2a30}", true),
        ("tint", "\u{222d}", true),
        ("toea", "\u{2928}", true),
        ("top", "\u{22a4}", true),
        ("topbot", "\u{2336}", true),
        ("topcir", "\u{2af1}", true),
        ("topf", "\u{1d565}", true),
        ("topfork", "\u{2ada}", true),
        ("tosa", "\u{2929}", true),
        ("tprime", "\u{2034}", true),
        ("trade", "\u{2122}", true),
        ("triangle", "\u{25b5}", true),
        ("triangledown", "\u{25bf}", true),
        ("triangleleft", "\u{25c3}", true),
        ("trianglelefteq", "\u{22b4}", true),
        ("triangleq", "\u{225c}", true),
        ("triangleright", "\u{25b9}", true),
        ("trianglerighteq", "\u{22b5}", true),
        ("tridot", "\u{25ec}", true),
        ("trie", "\u{225c}", true),
        ("triminus", "\u{2a3a}", true),
        ("triplus", "\u{2a39}", true),
        ("trisb", "\u{29cd}", true),
        ("tritime", "\u{2a3b}", true),
        ("trpezium", "\u{23e2}", true),
        ("tscr", "\u{1d4c9}", true),
        ("tscy", "\u{446}", true),
        ("tshcy", "\u{45b}", true),
        ("tstrok", "\u{167}", true),
        ("twixt", "\u{226c}", true),
        ("twoheadleftarrow", "\u{219e}", true),
        ("twoheadrightarrow", "\u{21a0}", true),
        ("uArr", "\u{21d1}", true),
        ("uHar", "\u{2963}", true),
        ("uacute", "\u{fa}", false),
        ("uarr", "\u{2191}", true),
        ("ubrcy", "\u{45e}", true),
        ("ubreve", "\u{16d}", true),
        ("ucirc", "\u{fb}", false),
        ("ucy", "\u{443}", true),
        ("udarr", "\u{21c5}", true),
        ("udblac", "\u{171}", true),
        ("udhar", "\u{296e}", true),
        ("ufisht", "\u{297e}", true),
        ("ufr", "\u{1d532}", true),
        ("ugrave", "\u{f9}", false),
        ("uharl", "\u{21bf}", true),
        ("uharr", "\u{21be}", true),
        ("uhblk", "\u{2580}", true),
        ("ulcorn", "\u{231c}", true),
        ("ulcorner", "\u{231c}", true),
        ("ulcrop", "\u{230f}", true),
        ("ultri", "\u{25f8}", true),
        ("umacr", "\u{16b}", true),
        ("uml", "\u{a8}", false),
        ("uogon", "\u{173}", true),
        ("uopf", "\u{1d566}", true),
        ("uparrow", "\u{2191}", true),
        ("updownarrow", "\u{2195}", true),
        ("upharpoonleft", "\u{21bf}", true),
        ("upharpoonright", "\u{21be}", true),
        ("uplus", "\u{228e}", true),
        ("upsi", "\u{3c5}", true),
        ("upsih", "\u{3d2}", true),
        ("upsilon", "\u{3c5}", true),
        ("upuparrows", "\u{21c8}", true),
        ("urcorn", "\u{231d}", true),
        ("urcorner", "\u{231d}", true),
        ("urcrop", "\u{230e}", true),
        ("uring", "\u{16f}", true),
        ("urtri", "\u{25f9}", true),
        ("uscr", "\u{1d4ca}", true),
        ("utdot", "\u{22f0}", true),
        ("utilde", "\u{169}", true),
        ("utri", "\u{25b5}", true),
        ("utrif", "\u{25b4}", true),
        ("uuarr", "\u{21c8}", true),
        ("uuml", "\u{fc}", false),
        ("uwangle", "\u{29a7}", true),
        ("vArr", "\u{21d5}", true),
        ("vBar", "\u{2ae8}", true),
        ("vBarv", "\u{2ae9}", true),
        ("vDash", "\u{22a8}", true),
        ("vangrt", "\u{299c}", true),
        ("varepsilon", "\u{3f5}", true),
        ("varkappa", "\u{3f0}", true),
        ("varnothing", "\u{2205}", true),
        ("varphi", "\u{3d5}", true),
        ("varpi", "\u{3d6}", true),
        ("varpropto", "\u{221d}", true),
        ("varr", "\u{2195}", true),
        ("varrho", "\u{3f1}", true),
        ("varsigma", "\u{3c2}", true),
        ("varsubsetneq", "\u{228a}\u{fe00}", true),
        ("varsubsetneqq", "\u{2acb}\u{fe00}", true),
        ("varsupsetneq", "\u{228b}\u{fe00}", true),
        ("varsupsetneqq", "\u{2acc}\u{fe00}", true),
        ("vartheta", "\u{3d1}", true),
        ("vartriangleleft", "\u{22b2}", true),
        ("vartriangleright", "\u{22b3}", true),
        ("vcy", "\u{432}", true),
        ("vdash", "\u{22a2}", true),
        ("vee", "\u{2228}", true),
        ("veebar", "\u{22bb}", true),
        ("veeeq", "\u{225a}", true),
        ("vellip", "\u{22ee}", true),
        ("verbar", "\u{7c}", true),
        ("vert", "\u{7c}", true),
        ("vfr", "\u{1d533}", true),
        ("vltri", "\u{22b2}", true),
        ("vnsub", "\u{2282}\u{20d2}", true),
        ("vnsup", "\u{2283}\u{20d2}", true),
        ("vopf", "\u{1d567}", true),
        ("vprop", "\u{221d}", true),
        ("vrtri", "\u{22b3}", true),
        ("vscr", "\u{1d4cb}", true),
        ("vsubnE", "\u{2acb}\u{fe00}", true),
        ("vsubne", "\u{228a}\u{fe00}", true),
        ("vsupnE", "\u{2acc}\u{fe00}", true),
        ("vsupne", "\u{228b}\u{fe00}", true),
        ("vzigzag", "\u{299a}", true),
        ("wcirc", "\u{175}", true),
        ("wedbar", "\u{2a5f}", true),
        ("wedge", "\u{2227}", true),
        ("wedgeq", "\u{2259}", true),
        ("weierp", "\u{2118}", true),
        ("wfr", "\u{1d534}", true),
        ("wopf", "\u{1d568}", true),
        ("wp", "\u{2118}", true),
        ("wr", "\u{2240}", true),
        ("wreath", "\u{2240}", true),
        ("wscr", "\u{1d4cc}", true),
        ("xcap", "\u{22c2}", true),
        ("xcirc", "\u{25ef}", true),
        ("xcup", "\u{22c3}", true),
        ("xdtri", "\u{25bd}", true),
        ("xfr", "\u{1d535}", true),
        ("xhArr", "\u{27fa}", true),
        ("xharr", "\u{27f7}", true),
        ("xi", "\u{3be}", true),
        ("xlArr", "\u{27f8}", true),
        ("xlarr", "\u{27f5}", true),
        ("xmap", "\u{27fc}", true),
        ("xnis", "\u{22fb}", true),
        ("xodot", "\u{2a00}", true),
        ("xopf", "\u{1d569}", true),
        ("xoplus", "\u{2a01}", true),
        ("xotime", "\u{2a02}", true),
        ("xrArr", "\u{27f9}", true),
        ("xrarr", "\u{27f6}", true),
        ("xscr", "\u{1d4cd}", true),
        ("xsqcup", "\u{2a06}", true),
        ("xuplus", "\u{2a04}", true),
        ("xutri", "\u{25b3}", true),
        ("xvee", "\u{22c1}", true),
        ("xwedge", "\u{22c0}", true),
        ("yacute", "\u{fd}", false),
        ("yacy", "\u{44f}", true),
        ("ycirc", "\u{177}", true),
        ("ycy", "\u{44b}", true),
        ("yen", "\u{a5}", false),
        ("yfr", "\u{1d536}", true),
        ("yicy", "\u{457}", true),
        ("yopf", "\u{1d56a}", true),
        ("yscr", "\u{1d4ce}", true),
        ("yucy", "\u{44e}", true),
        ("yuml", "\u{ff}", false),
        ("zacute", "\u{17a}", true),
        ("zcaron", "\u{17e}", true),
        ("zcy", "\u{437}", true),
        ("zdot", "\u{17c}", true),
        ("zeetrf", "\u{2128}", true),
        ("zeta", "\u{3b6}", true),
        ("zfr", "\u{1d537}", true),
        ("zhcy", "\u{436}", true),
        ("zigrarr", "\u{21dd}", true),
        ("zopf", "\u{1d56b}", true),
        ("zscr", "\u{1d4cf}", true),
        ("zwj", "\u{200d}", true),
        ("zwnj", "\u{200c}", true),
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
            for &(name, replacement, semicolon) in &ENTITIES {
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
