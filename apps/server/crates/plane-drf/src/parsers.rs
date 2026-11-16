// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! DRF's parsers (`rest_framework/parsers.py`), with Django's `QueryDict`.
//!
//! `request.data` is lazy. The first access runs `Request._parse`
//! (`rest_framework/request.py:330-384`):
//!
//! 1. `stream` is `None` when the declared `Content-Length` is 0 or absent
//!    (`_load_stream`, `request.py:301-318`), so an empty body is never parsed:
//!    `{}` for a non-form media type, an empty `QueryDict` for a form type.
//! 2. `request.content_type` is the raw `CONTENT_TYPE` header, or `""`. An
//!    empty `Content-Type` with a body therefore reaches the parser selection
//!    and is refused with `Unsupported media type "" in request.`
//! 3. `select_parser` picks the first parser in `view.parser_classes` whose
//!    `media_type` matches, refusing a `Content-Type` longer than 256 bytes.
//! 4. The parser runs. A `FormParser` produces a `QueryDict`
//!    (`DATA_UPLOAD_MAX_NUMBER_FIELDS` applies), a `MultiPartParser` a
//!    `DataAndFiles` pair, and a `JSONParser` a Python JSON tree.
//!
//! ## JSON
//!
//! DRF's `JSONParser` is `json.loads` with CPython's exact error text. That is
//! `plane_serde::pyjson::loads`, and [`crate::error::ViewError`] carries the
//! two outcomes DRF distinguishes: a `ValueError` becomes
//! `ParseError("JSON parse error - …")` (400) and a `RecursionError` is not
//! caught, so the family `handle_exception` answers 500.
//!
//! The charset is resolved the way Django resolves it first:
//! `HttpRequest._set_content_type_params` (`django/http/request.py:152-163`)
//! keeps `DEFAULT_CHARSET` unless the `charset` parameter names a codec Python
//! knows, so `charset=nonsense` is parsed as UTF-8 and only a *known non-text*
//! codec (`bz2`, `base64`, …) reaches DRF's `get_encoding` and is refused.
//!
//! ## Remaining gaps
//!
//! * **Multipart files are skipped**, not stored: only non-file field values
//!   are exposed and `request.FILES` stays empty. `QueryDict::has_files`
//!   records that a file part was present, which is enough for a handler that
//!   publishes `request.data` to reproduce Django's encoder `TypeError`.
//!   Django's `MultiPartParserError` wording and
//!   `DATA_UPLOAD_MAX_NUMBER_FIELDS` counting for parts are not reproduced.
//! * `MultiPartParser` is only selected when the view lists it; Plane's asset
//!   endpoints do.
//! * `pyjson` decodes UTF-8/ASCII/latin-1/cp1252/UTF-16/UTF-32. A request body
//!   in another text encoding (`cp437`, `koi8-r`) is refused where Django
//!   decodes it; see `plane_serde::pyjson::loads`. Plane clients send UTF-8.

use plane_core::context::RequestContext;
use plane_core::error::ApiError;
use plane_serde::pyjson;
use plane_serde::pyjson::Value as JsonValue;

use crate::error::ViewError;
use crate::negotiate::{
    MAX_MEDIA_TYPE_LENGTH, MediaType, is_form_media_type, parse_header_parameters,
};
use crate::request::{DATA_UPLOAD_MAX_NUMBER_FIELDS, DEFAULT_CHARSET, QueryDict, TooManyFields};

/// `request.data` after parsing.
#[derive(Debug, Clone, PartialEq)]
pub enum Data {
    /// `JSONParser`'s return value: a `json.loads` tree (object, array or
    /// scalar). An empty non-form body is an empty object.
    Json(JsonValue),
    /// A form body (urlencoded or multipart), with `QueryDict` semantics.
    Form(QueryDict),
}

impl Data {
    pub fn empty_object() -> Self {
        Self::Json(JsonValue::Dict(pyjson::PyDict::new()))
    }

    pub fn empty_form() -> Self {
        Self::Form(QueryDict::default())
    }

    /// `request.data.get(key)` for a form; a JSON object's field otherwise.
    pub fn get(&self, key: &str) -> Option<&str> {
        match self {
            Self::Form(query) => query.get(key),
            Self::Json(value) => value.get(key).and_then(JsonValue::as_str),
        }
    }

    pub fn getlist(&self, key: &str) -> Vec<&str> {
        match self {
            Self::Form(query) => query.getlist(key),
            Self::Json(_) => self.get(key).into_iter().collect(),
        }
    }

    /// `isinstance(request.data, (dict, list))`.
    pub fn is_container(&self) -> bool {
        match self {
            Self::Form(_) => true,
            Self::Json(value) => matches!(value, JsonValue::Dict(_) | JsonValue::List(_)),
        }
    }

    /// The parsed JSON tree, when the body was JSON.
    pub fn json(&self) -> Option<&JsonValue> {
        match self {
            Self::Json(value) => Some(value),
            Self::Form(_) => None,
        }
    }
}

/// `Request._parse` (`request.py:330-384`).
pub fn parse(ctx: &RequestContext, parser_classes: &[String]) -> Result<Data, ViewError> {
    // `_load_stream`: `content_length == 0` means "no stream".
    let content_length = ctx
        .header("content-length")
        .and_then(|value| value.trim().parse::<i64>().ok())
        .unwrap_or(0);
    let raw_content_type = ctx.header("content-type").unwrap_or("");
    let media_type = if raw_content_type.len() > MAX_MEDIA_TYPE_LENGTH {
        None
    } else {
        Some(media_type_of(raw_content_type))
    };

    if content_length == 0 {
        // `stream is None`: an empty body, typed by the media type only.
        return Ok(if is_form_media_type(raw_content_type) {
            Data::empty_form()
        } else {
            Data::empty_object()
        });
    }

    let Some((parser, charset)) = select_parser(parser_classes, raw_content_type, media_type)
    else {
        return Err(ViewError::Api(ApiError::UnsupportedMediaType(
            raw_content_type.to_string(),
        )));
    };

    let body = ctx.body.as_deref().unwrap_or(&[]);
    match parser {
        ParserKind::Json => match pyjson::loads(body, &charset) {
            Ok(value) => Ok(Data::Json(value)),
            Err(error) => match error.parse_error_detail() {
                Some(detail) => Err(ViewError::Api(ApiError::ParseError(detail))),
                // `RecursionError` is not a `ValueError`; DRF re-raises it and
                // the family `handle_exception` answers 500.
                None => Err(ViewError::Unknown(error.message().to_string())),
            },
        },
        ParserKind::Form => {
            let text = decode_text(body, &charset)?;
            let query = QueryDict::parse(&text, Some(DATA_UPLOAD_MAX_NUMBER_FIELDS))
                .map_err(|TooManyFields| ViewError::TooManyFields)?;
            Ok(Data::Form(query))
        }
        ParserKind::MultiPart => parse_multipart(body, raw_content_type, charset),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParserKind {
    Json,
    Form,
    MultiPart,
}

impl ParserKind {
    fn from_dotted(class: &str) -> Option<Self> {
        match class {
            "rest_framework.parsers.JSONParser" => Some(Self::Json),
            "rest_framework.parsers.FormParser" => Some(Self::Form),
            "rest_framework.parsers.MultiPartParser" => Some(Self::MultiPart),
            _ => None,
        }
    }

    fn media_type(self) -> &'static str {
        match self {
            Self::Json => "application/json",
            Self::Form => "application/x-www-form-urlencoded",
            Self::MultiPart => "multipart/form-data",
        }
    }
}

/// `DefaultContentNegotiation.select_parser` (`negotiation.py:29-41`).
///
/// The first parser whose media type matches wins; a media type longer than
/// 256 characters returns `None`, which becomes a 415.
fn select_parser(
    parser_classes: &[String],
    raw_content_type: &str,
    media_type: Option<MediaType>,
) -> Option<(ParserKind, String)> {
    let media_type = media_type?;
    if raw_content_type.is_empty() {
        // DRF matches the parser's media type against `""`, which never
        // matches; the 415 body then names `""`, not `"None"`.
        return None;
    }
    for class in parser_classes {
        let Some(parser) = ParserKind::from_dotted(class) else {
            continue;
        };
        if MediaType::parse(parser.media_type()).matches(&media_type) {
            let charset = media_type
                .params
                .get("charset")
                .map(|raw| resolve_charset(raw))
                .unwrap_or_else(|| DEFAULT_CHARSET.to_string());
            return Some((parser, charset));
        }
    }
    None
}

/// `request.content_type`: the media type *before* the parameters.
fn media_type_of(raw: &str) -> MediaType {
    MediaType::parse(raw)
}

/// Django's `HttpRequest._set_content_type_params` (`request.py:152-163`):
/// `request.encoding` becomes the `charset` parameter only when
/// `codecs.lookup()` resolves it; otherwise it stays `DEFAULT_CHARSET`.
///
/// The list below is the set of Python codecs this port and `pyjson` know
/// about. A name outside it is treated as "Python does not know it" and falls
/// back to UTF-8, which is Django's behaviour for a typo or an unknown value.
/// A known text codec `pyjson` cannot decode (`cp437`) is passed through and
/// refused by `pyjson`, its documented limitation.
fn resolve_charset(raw: &str) -> String {
    let name = raw.trim().to_ascii_lowercase();
    let known = matches!(
        name.as_str(),
        // Text codecs (supported by pyjson, plus common aliases).
        "utf-8" | "utf8" | "u8" | "utf" | "utf-8-sig"
            | "ascii" | "us-ascii" | "646"
            | "latin-1" | "latin1" | "iso-8859-1" | "iso8859-1" | "l1" | "cp819" | "8859"
            | "cp1252" | "windows-1252"
            | "utf-16" | "utf16" | "utf-16-le" | "utf-16-be"
            | "utf-32" | "utf32" | "utf-32-le" | "utf-32-be"
            // Common text codecs Python resolves but pyjson refuses.
            | "cp437" | "cp850" | "koi8-r" | "koi8-u" | "mac-roman"
            | "iso-8859-2" | "iso-8859-5" | "iso-8859-7" | "iso-8859-15"
            | "windows-1250" | "windows-1251" | "windows-1254" | "windows-1255"
            | "gb2312" | "gbk" | "big5" | "shift_jis" | "euc-jp" | "euc-kr" | "tis-620"
            // Known non-text codecs: kept so DRF's `get_encoding` refuses them.
            | "bz2" | "bz2_codec" | "base64" | "base64_codec" | "quopri" | "quopri_codec"
            | "uu" | "uu_codec" | "hex" | "hex_codec" | "rot13" | "rot_13" | "zlib" | "zlib_codec"
    );
    if known {
        name
    } else {
        DEFAULT_CHARSET.to_string()
    }
}

/// `get_encoding` + `bytes.decode(charset)`.
///
/// Python accepts any codec `codecs.lookup` resolves and rejects the
/// bytes-to-bytes codecs (`bz2`, `base64`, …) with a `ParseError`. Only the
/// text encodings a client can sensibly send are supported; anything else gets
/// the same `ParseError` wording. A body that is not valid in the declared
/// encoding raises `UnicodeDecodeError`, which DRF does not wrap.
fn decode_text(body: &[u8], charset: &str) -> Result<String, ViewError> {
    let normalized = charset.trim().to_ascii_lowercase();
    if normalized.contains("bz2")
        || normalized.contains("base64")
        || normalized.contains("zlib")
        || normalized.contains("quopri")
        || normalized.contains("uu")
        || normalized.contains("hex")
        || normalized.contains("rot13")
    {
        return Err(ViewError::Api(ApiError::ParseError(format!(
            "Unsupported charset \"{charset}\" in request Content-Type header."
        ))));
    }
    match normalized.as_str() {
        "utf-8" | "utf8" | "u8" | "utf" => std::str::from_utf8(body)
            .map(str::to_string)
            .map_err(|error| ViewError::Unknown(format!("UnicodeDecodeError: {error}"))),
        "ascii" | "us-ascii" | "646" => {
            if body.is_ascii() {
                Ok(String::from_utf8_lossy(body).into_owned())
            } else {
                Err(ViewError::Unknown(
                    "UnicodeDecodeError: 'ascii' codec can't decode byte".to_string(),
                ))
            }
        }
        "latin-1" | "latin1" | "iso-8859-1" | "iso8859-1" | "l1" | "cp819" | "8859" => {
            Ok(body.iter().map(|byte| char::from(*byte)).collect())
        }
        other => Err(ViewError::Api(ApiError::ParseError(format!(
            "Unsupported charset \"{other}\" in request Content-Type header."
        )))),
    }
}

/// A minimal RFC 7578 multipart parser: non-file field values only.
///
/// Django's `MultiPartParser` feeds chunks to the upload handlers and decodes
/// each part with the request charset; this port does the same for fields and
/// **skips file parts**, recording only that one was present
/// ([`QueryDict::has_files`]) so a handler that publishes `request.data` can
/// reproduce Django's `TypeError: Object of type … is not JSON serializable`.
/// Two Django behaviours are deliberately not reproduced and are listed in the
/// module docs: the `DATA_UPLOAD_MAX_NUMBER_FIELDS` count for parts, and the
/// exact `MultiPartParserError` wording beyond a missing boundary.
fn parse_multipart(
    body: &[u8],
    raw_content_type: &str,
    charset: String,
) -> Result<Data, ViewError> {
    let (_, params) = parse_header_parameters(raw_content_type);
    let Some(boundary) = params.get("boundary").filter(|value| !value.is_empty()) else {
        return Err(ViewError::Api(ApiError::ParseError(
            "Multipart form parse error - Invalid boundary in multipart form: None".to_string(),
        )));
    };
    let delimiter = format!("--{boundary}");
    let mut query = QueryDict::default();
    // The body is `--boundary\r\npart...\r\n--boundary...`; split on the
    // delimiter and parse everything between.
    let text = match std::str::from_utf8(body) {
        Ok(text) => text.to_string(),
        Err(_) => decode_text(body, &charset)?,
    };
    let mut rest = text.as_str();
    if let Some(index) = rest.find(&delimiter) {
        rest = &rest[index + delimiter.len()..];
    } else {
        return Err(ViewError::Api(ApiError::ParseError(
            "Multipart form parse error - Invalid boundary in multipart form".to_string(),
        )));
    }
    loop {
        // `--` after a delimiter closes the body.
        if rest.starts_with("--") {
            break;
        }
        // A CRLF follows a delimiter; the part starts after it.
        rest = rest.strip_prefix("\r\n").unwrap_or(rest);
        let Some(end) = rest.find(&format!("\r\n{delimiter}")) else {
            break;
        };
        let part = &rest[..end];
        // `end` points at the CRLF before the next `--boundary`; skip both so
        // the loop's closing-delimiter check sees the `--` of `--boundary--`
        // (and the next iteration starts after the delimiter, like the first).
        rest = &rest[end + 2..];
        rest = rest.strip_prefix(delimiter.as_str()).unwrap_or(rest);
        let Some((headers, value)) = part.split_once("\r\n\r\n") else {
            continue;
        };
        let mut name = None;
        let mut is_file = false;
        for line in headers.split("\r\n") {
            let Some((header, header_value)) = line.split_once(':') else {
                continue;
            };
            if !header.trim().eq_ignore_ascii_case("content-disposition") {
                continue;
            }
            let (_, disposition) = parse_header_parameters(header_value);
            name = disposition.get("name").cloned();
            is_file = disposition.contains_key("filename");
        }
        if is_file {
            query.mark_file(name.as_deref());
            continue;
        }
        let Some(name) = name else {
            continue;
        };
        let value = decode_text(value.as_bytes(), &charset)?;
        query.push(name, value);
    }
    Ok(Data::Form(query))
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::{HeaderMap, Method};

    fn ctx(headers: &[(&str, &str)], body: Option<&str>) -> RequestContext {
        let mut map = HeaderMap::new();
        for (name, value) in headers {
            map.insert(
                http::HeaderName::from_bytes(name.as_bytes()).expect("name"),
                http::HeaderValue::from_str(value).expect("value"),
            );
        }
        let mut ctx = RequestContext::new(Method::POST, "/", "", map, None);
        ctx.body = body.map(|body| body.as_bytes().to_vec());
        ctx
    }

    const PARSERS: &[&str] = &[
        "rest_framework.parsers.JSONParser",
        "rest_framework.parsers.FormParser",
        "rest_framework.parsers.MultiPartParser",
    ];

    fn parsers() -> Vec<String> {
        PARSERS.iter().map(|class| (*class).to_string()).collect()
    }

    #[test]
    fn multipart_file_parts_are_flagged() {
        let body = concat!(
            "--BOUND\r\nContent-Disposition: form-data; name=\"name\"\r\n\r\nhello\r\n",
            "--BOUND\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a.txt\"\r\n",
            "Content-Type: text/plain\r\n\r\nabc\r\n",
            "--BOUND--\r\n"
        );
        let length = body.len().to_string();
        let data = parse(
            &ctx(
                &[
                    ("content-type", "multipart/form-data; boundary=BOUND"),
                    ("content-length", &length),
                ],
                Some(body),
            ),
            &parsers(),
        )
        .expect("multipart parses");
        match data {
            Data::Form(query) => {
                assert_eq!(query.get("name"), Some("hello"), "parsed {query:?}");
                assert!(query.has_files(), "the file part must be recorded");
                assert!(query.is_file("file"), "the file part's name is kept");
                assert_eq!(query.file_keys(), ["file".to_string()]);
                assert!(!query.is_file("name"));
            }
            other => panic!("expected a form body, got {other:?}"),
        }
    }

    #[test]
    fn urlencoded_bodies_have_no_files() {
        let data = parse(
            &ctx(
                &[("content-type", "application/x-www-form-urlencoded")],
                Some("name=hello"),
            ),
            &parsers(),
        )
        .expect("urlencoded parses");
        match data {
            Data::Form(query) => assert!(!query.has_files()),
            other => panic!("expected a form body, got {other:?}"),
        }
    }

    #[test]
    fn empty_body_is_never_parsed() {
        // Non-form content type: `{}`.
        assert_eq!(
            parse(
                &ctx(&[("content-type", "application/json")], None),
                &parsers()
            ),
            Ok(Data::empty_object())
        );
        // Form content type: an empty QueryDict.
        assert_eq!(
            parse(
                &ctx(
                    &[("content-type", "application/x-www-form-urlencoded")],
                    None
                ),
                &parsers()
            ),
            Ok(Data::empty_form())
        );
        // Declared length 0 with bytes present: still no parse.
        assert_eq!(
            parse(
                &ctx(
                    &[
                        ("content-type", "application/json"),
                        ("content-length", "0")
                    ],
                    Some("{\"a\":1}")
                ),
                &parsers()
            ),
            Ok(Data::empty_object())
        );
    }

    #[test]
    fn unsupported_media_type_wording() {
        // text/plain with a body: 415 naming the header value.
        assert_eq!(
            parse(
                &ctx(
                    &[("content-type", "text/plain"), ("content-length", "4")],
                    Some("body")
                ),
                &parsers()
            ),
            Err(ViewError::Api(ApiError::UnsupportedMediaType(
                "text/plain".to_string()
            )))
        );
        // No Content-Type with a body: names "".
        assert_eq!(
            parse(&ctx(&[("content-length", "4")], Some("body")), &parsers()),
            Err(ViewError::Api(
                ApiError::UnsupportedMediaType(String::new())
            ))
        );
        // A parser list without JSONParser refuses application/json.
        let form_only = vec![
            "rest_framework.parsers.MultiPartParser".to_string(),
            "rest_framework.parsers.FormParser".to_string(),
        ];
        assert_eq!(
            parse(
                &ctx(
                    &[
                        ("content-type", "application/json"),
                        ("content-length", "2")
                    ],
                    Some("{}")
                ),
                &form_only
            ),
            Err(ViewError::Api(ApiError::UnsupportedMediaType(
                "application/json".to_string()
            )))
        );
    }

    #[test]
    fn urlencoded_bodies_use_querydict() {
        let data = parse(
            &ctx(
                &[
                    ("content-type", "application/x-www-form-urlencoded"),
                    ("content-length", "15"),
                ],
                Some("a=1&b=2&a=3&c=x"),
            ),
            &parsers(),
        )
        .expect("parse");
        let Data::Form(query) = data else {
            panic!("expected a form");
        };
        assert_eq!(query.get("a"), Some("3"));
        assert_eq!(query.getlist("a"), vec!["1", "3"]);
        assert_eq!(query.get("c"), Some("x"));
    }

    #[test]
    fn form_field_limit_is_django_500() {
        let body = (0..=DATA_UPLOAD_MAX_NUMBER_FIELDS)
            .map(|index| format!("f{index}=1"))
            .collect::<Vec<_>>()
            .join("&");
        let length = body.len().to_string();
        assert_eq!(
            parse(
                &ctx(
                    &[
                        ("content-type", "application/x-www-form-urlencoded"),
                        ("content-length", &length),
                    ],
                    Some(&body),
                ),
                &parsers()
            ),
            Err(ViewError::TooManyFields)
        );
    }

    #[test]
    fn charset_rules() {
        // latin-1 decodes byte for byte.
        assert_eq!(
            decode_text(&[0x61, 0x3d, 0xff], "latin-1").expect("latin-1"),
            "a=\u{ff}"
        );
        // A non-text codec is a ParseError naming it.
        assert_eq!(
            parse(
                &ctx(
                    &[
                        ("content-type", "application/json; charset=bz2"),
                        ("content-length", "2"),
                    ],
                    Some("{}")
                ),
                &parsers()
            ),
            Err(ViewError::Api(ApiError::ParseError(
                "Unsupported charset \"bz2\" in request Content-Type header.".to_string()
            )))
        );
    }

    #[test]
    fn json_bodies_use_python_semantics() {
        let data = parse(
            &ctx(
                &[
                    ("content-type", "application/json"),
                    ("content-length", "13"),
                ],
                Some("{\"a\": 1, \"b\": \"x\"}"),
            ),
            &parsers(),
        )
        .expect("parse");
        let value = data.json().expect("json");
        assert_eq!(value.get("b").and_then(JsonValue::as_str), Some("x"));
        assert!(data.is_container());

        // A JSON scalar parses; `/api/v1/`'s container check then rejects it,
        // and `/api/` handlers see a scalar.
        let scalar = parse(
            &ctx(
                &[
                    ("content-type", "application/json"),
                    ("content-length", "4"),
                ],
                Some("null"),
            ),
            &parsers(),
        )
        .expect("parse");
        assert!(!scalar.is_container());
        assert_eq!(scalar.json(), Some(&JsonValue::Null));
    }

    #[test]
    fn json_parse_errors_are_cpythons() {
        let error = parse(
            &ctx(
                &[
                    ("content-type", "application/json"),
                    ("content-length", "5"),
                ],
                Some("{\"a\":"),
            ),
            &parsers(),
        )
        .expect_err("must fail");
        assert_eq!(
            error,
            ViewError::Api(ApiError::ParseError(
                "JSON parse error - Expecting value: line 1 column 6 (char 5)".to_string()
            ))
        );

        // `strict_constant`: NaN is refused by DRF's JSONParser.
        let error = parse(
            &ctx(
                &[
                    ("content-type", "application/json"),
                    ("content-length", "9"),
                ],
                Some("{\"x\": NaN}"),
            ),
            &parsers(),
        )
        .expect_err("must fail");
        assert!(
            matches!(
                &error,
                ViewError::Api(ApiError::ParseError(detail))
                    if detail.starts_with("JSON parse error - Out of range float values")
            ),
            "{error:?}"
        );
    }

    #[test]
    fn unknown_charsets_fall_back_to_utf8() {
        // Django's `_set_content_type_params` never forwards a charset Python
        // cannot resolve; DRF therefore never sees `nonsense`.
        assert_eq!(resolve_charset("nonsense"), "utf-8");
        assert_eq!(resolve_charset("UTF-8"), "utf-8");
        assert_eq!(resolve_charset("latin-1"), "latin-1");
        // A known non-text codec is forwarded and refused by DRF.
        assert_eq!(resolve_charset("bz2"), "bz2");
        assert_eq!(
            parse(
                &ctx(
                    &[
                        ("content-type", "application/json; charset=nonsense"),
                        ("content-length", "2"),
                    ],
                    Some("{}")
                ),
                &parsers()
            ),
            Ok(Data::Json(JsonValue::Dict(pyjson::PyDict::new())))
        );
        assert_eq!(
            parse(
                &ctx(
                    &[
                        ("content-type", "application/json; charset=bz2"),
                        ("content-length", "2"),
                    ],
                    Some("{}")
                ),
                &parsers()
            ),
            Err(ViewError::Api(ApiError::ParseError(
                "Unsupported charset \"bz2\" in request Content-Type header.".to_string()
            )))
        );
    }
}
