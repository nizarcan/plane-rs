// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The XLSX writer Plane needs, byte-compatible with openpyxl 3.1.2.
//!
//! `plane/utils/porters/formatters.py:220-276` builds a workbook with
//! `openpyxl.Workbook()`, `ws.append(...)` and `wb.save(BytesIO())`. The
//! export task then puts those bytes in a ZIP, so the file a user downloads
//! must (a) open in Excel and (b) be provably the same document the Python
//! worker produced. This crate reproduces openpyxl's **part set, part order and
//! XML** exactly, and its cell model:
//!
//! * strings are inline strings (`t="inlineStr"`), never a shared-string
//!   table — openpyxl is created with `write_only=False`, and its writer still
//!   emits inline strings for ordinary append-ed values;
//! * `int`/`float` are numeric cells, `bool` is `t="b"`, an empty string is an
//!   empty inline string, and a string in openpyxl's error-code set is
//!   `t="e"`;
//! * a string longer than 32,767 code points is truncated, and a string with
//!   one of openpyxl's illegal control characters
//!   (`[\x00-\x08\x0b\x0c\x0e-\x1f]`) raises `IllegalCharacterError` before
//!   anything is written (`openpyxl/cell/cell.py:45,153-165`);
//! * text is XML-escaped as `&`, `<`, `>` and serialised to ASCII, with every
//!   non-ASCII character as a decimal character reference (`&#233;`), exactly
//!   as the lxml serialiser openpyxl uses; leading/trailing whitespace adds
//!   `xml:space="preserve"` (`openpyxl/xml/functions.py:84-87`).
//!
//! The timestamp families are injected: the ZIP's DOS entry times and
//! `docProps/core.xml`'s `created`/`modified` both come from the clock the
//! caller passes. `difftest task` normalises exactly those two families and
//! compares everything else (see `plans/CUTOVER.md` §2 and D27).
//!
//! The static parts live in `templates/` as the exact bytes openpyxl 3.1.2
//! emitted: the theme, styles, both relationship files, `[Content_Types].xml`,
//! `docProps/app.xml` and `xl/workbook.xml`. `tools/inventory/xlsx_corpus.py`
//! regenerates the golden corpus (five row sets, every entry decompressed and
//! compared in order) from the pinned `apps/api/.venv`; `tests/golden.rs`
//! compares this crate against it.

use std::io::Write as _;

use chrono::{DateTime, Datelike, Timelike, Utc};
use flate2::Compression;
use flate2::write::DeflateEncoder;

/// One cell, restricted to the types the exporter produces.
#[derive(Clone, Debug, PartialEq)]
pub enum Cell {
    /// A string; openpyxl's `check_string` truncation and character rules
    /// apply.
    Text(String),
    /// An integer (`t="n"`).
    Int(i64),
    /// A float (`t="n"`), written with C's `%.16g`.
    Float(f64),
    /// A boolean (`t="b"`).
    Bool(bool),
}

/// Why a workbook could not be written.
#[derive(Debug, thiserror::Error)]
pub enum XlsxError {
    /// openpyxl raises `IllegalCharacterError` from `Cell.value`; the task's
    /// `except Exception` turns it into `ExporterHistory.reason`, so the
    /// message must be Python's.
    #[error("{0} cannot be used in worksheets.")]
    IllegalCharacter(String),
    /// A deflate writer failure (an allocation problem in practice).
    #[error("compressing {0}: {1}")]
    Compression(String, std::io::Error),
}

/// The parts openpyxl 3.1.2 writes, in the order `ExcelWriter.write_data`
/// appends them (`openpyxl/writer/excel.py:67-107`).
const STATIC_PARTS: [(&str, &str); 7] = [
    (
        "docProps/app.xml",
        include_str!("../templates/docProps__app.xml"),
    ),
    (
        "xl/theme/theme1.xml",
        include_str!("../templates/xl__theme__theme1.xml"),
    ),
    ("xl/styles.xml", include_str!("../templates/xl__styles.xml")),
    ("_rels/.rels", include_str!("../templates/_rels__.rels")),
    (
        "xl/workbook.xml",
        include_str!("../templates/xl__workbook.xml"),
    ),
    (
        "xl/_rels/workbook.xml.rels",
        include_str!("../templates/xl___rels__workbook.xml.rels"),
    ),
    (
        "[Content_Types].xml",
        include_str!("../templates/[Content_Types].xml"),
    ),
];

/// `openpyxl.cell.cell.ERROR_CODES`: a string equal to one of these becomes an
/// error cell.
const ERROR_CODES: [&str; 7] = [
    "#NULL!", "#DIV/0!", "#VALUE!", "#REF!", "#NAME?", "#NUM!", "#N/A",
];

/// openpyxl's 32,767-character cell limit (`cell.py:163`).
const MAX_CELL_CHARS: usize = 32767;

/// Render one workbook exactly as `Workbook().active` + `ws.append(...)` +
/// `wb.save()` would for `rows`, with `now` as both timestamp source.
///
/// # Errors
///
/// [`XlsxError::IllegalCharacter`] for a string openpyxl refuses, and
/// [`XlsxError::Compression`] if deflating fails.
pub fn workbook(rows: &[Vec<Cell>], now: DateTime<Utc>) -> Result<Vec<u8>, XlsxError> {
    let sheet = sheet_xml(rows)?;
    let timestamp = core_timestamp(now);
    let core = core_xml(&timestamp);

    let entries: Vec<(&str, Vec<u8>)> = vec![
        ("docProps/app.xml", STATIC_PARTS[0].1.as_bytes().to_vec()),
        ("docProps/core.xml", core.into_bytes()),
        ("xl/theme/theme1.xml", STATIC_PARTS[1].1.as_bytes().to_vec()),
        ("xl/worksheets/sheet1.xml", sheet.into_bytes()),
        ("xl/styles.xml", STATIC_PARTS[2].1.as_bytes().to_vec()),
        ("_rels/.rels", STATIC_PARTS[3].1.as_bytes().to_vec()),
        ("xl/workbook.xml", STATIC_PARTS[4].1.as_bytes().to_vec()),
        (
            "xl/_rels/workbook.xml.rels",
            STATIC_PARTS[5].1.as_bytes().to_vec(),
        ),
        ("[Content_Types].xml", STATIC_PARTS[6].1.as_bytes().to_vec()),
    ];
    Ok(zip(&entries, now))
}

/// `docProps/core.xml` as `openpyxl.packaging.core.DocumentProperties` writes
/// it. openpyxl calls `datetime.now(timezone.utc)` twice; the two calls agree
/// in practice and this port uses one clock instant for both.
#[must_use]
pub fn core_xml(timestamp: &str) -> String {
    format!(
        concat!(
            "<cp:coreProperties ",
            "xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\">",
            "<dc:creator xmlns:dc=\"http://purl.org/dc/elements/1.1/\">openpyxl</dc:creator>",
            "<dcterms:created xmlns:dcterms=\"http://purl.org/dc/terms/\" ",
            "xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" ",
            "xsi:type=\"dcterms:W3CDTF\">{0}</dcterms:created>",
            "<dcterms:modified xmlns:dcterms=\"http://purl.org/dc/terms/\" ",
            "xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" ",
            "xsi:type=\"dcterms:W3CDTF\">{0}</dcterms:modified>",
            "</cp:coreProperties>"
        ),
        timestamp
    )
}

/// `datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")`.
#[must_use]
pub fn core_timestamp(now: DateTime<Utc>) -> String {
    now.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// `xl/worksheets/sheet1.xml` for the given rows.
///
/// # Errors
///
/// [`XlsxError::IllegalCharacter`].
pub fn sheet_xml(rows: &[Vec<Cell>]) -> Result<String, XlsxError> {
    let mut out = String::with_capacity(4096 + rows.len() * 256);
    out.push_str("<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\">");
    out.push_str(
        "<sheetPr><outlinePr summaryBelow=\"1\" summaryRight=\"1\"/><pageSetUpPr/></sheetPr>",
    );
    out.push_str("<dimension ref=\"");
    out.push_str(&dimension(rows));
    out.push_str("\"/>");
    out.push_str(
        "<sheetViews><sheetView workbookViewId=\"0\"><selection activeCell=\"A1\" sqref=\"A1\"/></sheetView></sheetViews>",
    );
    out.push_str("<sheetFormatPr baseColWidth=\"8\" defaultRowHeight=\"15\"/>");
    out.push_str("<sheetData>");
    for (row_index, row) in rows.iter().enumerate() {
        if row.is_empty() {
            continue;
        }
        let row_number = row_index + 1;
        out.push_str("<row r=\"");
        out.push_str(&row_number.to_string());
        out.push_str("\">");
        for (column_index, cell) in row.iter().enumerate() {
            let coordinate = format!("{}{row_number}", column_name(column_index));
            write_cell(&mut out, &coordinate, cell)?;
        }
        out.push_str("</row>");
    }
    out.push_str("</sheetData>");
    out.push_str(
        "<pageMargins left=\"0.75\" right=\"0.75\" top=\"1\" bottom=\"1\" header=\"0.5\" footer=\"0.5\"/>",
    );
    out.push_str("</worksheet>");
    Ok(out)
}

fn write_cell(out: &mut String, coordinate: &str, cell: &Cell) -> Result<(), XlsxError> {
    match cell {
        Cell::Text(value) => {
            let value = truncate(value);
            if illegal_character(&value).is_some() {
                return Err(XlsxError::IllegalCharacter(value));
            }
            if ERROR_CODES.contains(&value.as_str()) {
                out.push_str("<c r=\"");
                out.push_str(coordinate);
                out.push_str("\" t=\"e\"><v>");
                push_xml_text(out, &value);
                out.push_str("</v></c>");
                return Ok(());
            }
            if value.len() > 1 && value.starts_with('=') {
                out.push_str("<c r=\"");
                out.push_str(coordinate);
                out.push_str("\"><f>");
                push_xml_text(out, &value[1..]);
                out.push_str("</f><v></v></c>");
                return Ok(());
            }
            out.push_str("<c r=\"");
            out.push_str(coordinate);
            out.push_str("\" t=\"inlineStr\">");
            if !value.is_empty() {
                out.push_str("<is><t");
                if value.trim() != value {
                    out.push_str(" xml:space=\"preserve\"");
                }
                out.push('>');
                push_xml_text(out, &value);
                out.push_str("</t></is>");
            }
            out.push_str("</c>");
        }
        Cell::Int(value) => {
            // openpyxl runs every numeric cell through `safe_string`, which
            // is `%.16g` on the float conversion — a 17-digit integer loses
            // its last digit exactly as it does here.
            out.push_str("<c r=\"");
            out.push_str(coordinate);
            out.push_str("\" t=\"n\"><v>");
            out.push_str(&percent_g(*value as f64, 16));
            out.push_str("</v></c>");
        }
        Cell::Float(value) => {
            out.push_str("<c r=\"");
            out.push_str(coordinate);
            out.push_str("\" t=\"n\"><v>");
            out.push_str(&percent_g(*value, 16));
            out.push_str("</v></c>");
        }
        Cell::Bool(value) => {
            out.push_str("<c r=\"");
            out.push_str(coordinate);
            out.push_str("\" t=\"b\"><v>");
            out.push_str(if *value { "1" } else { "0" });
            out.push_str("</v></c>");
        }
    }
    Ok(())
}

/// `value[:32767]`, by code point like Python.
fn truncate(value: &str) -> String {
    value.chars().take(MAX_CELL_CHARS).collect()
}

fn illegal_character(value: &str) -> Option<char> {
    value.chars().find(|character| {
        matches!(*character, '\u{0}'..='\u{8}' | '\u{b}'..='\u{c}' | '\u{e}'..='\u{1f}')
    })
}

/// openpyxl's sheet dimension (`calculate_dimension`): the bounding box of the
/// cells, `A1:A1` for a sheet with none.
fn dimension(rows: &[Vec<Cell>]) -> String {
    let mut last_row = 0usize;
    let mut last_column = 0usize;
    for (row_index, row) in rows.iter().enumerate() {
        if row.is_empty() {
            continue;
        }
        last_row = row_index + 1;
        last_column = last_column.max(row.len());
    }
    if last_row == 0 {
        return "A1:A1".to_string();
    }
    format!("A1:{}{last_row}", column_name(last_column - 1))
}

/// 0-based column index to Excel letters (0 -> A, 26 -> AA).
fn column_name(index: usize) -> String {
    let mut index = index + 1;
    let mut name = String::new();
    while index > 0 {
        let remainder = (index - 1) % 26;
        name.insert(0, char::from(b'A' + remainder as u8));
        index = (index - 1) / 26;
    }
    name
}

/// XML text escaping as lxml does it with an ASCII output encoding: `&`, `<`
/// and `>` are entities, and every character above U+007F is a decimal
/// character reference.
fn push_xml_text(out: &mut String, value: &str) {
    for character in value.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\t' | '\n' => out.push(character),
            // A literal CR would be normalised to LF by an XML parser, so the
            // serialiser writes a reference.
            '\r' => out.push_str("&#13;"),
            '\u{20}'..='\u{7e}' => out.push(character),
            _ => {
                out.push_str("&#");
                out.push_str(&u32::from(character).to_string());
                out.push(';');
            }
        }
    }
}

/// C's `%.*g` for a finite double, which is how openpyxl writes numbers
/// (`openpyxl/compat/strings.py:12-25`).
fn percent_g(value: f64, precision: usize) -> String {
    if value == 0.0 {
        return if value.is_sign_negative() {
            "-0".to_string()
        } else {
            "0".to_string()
        };
    }
    let precision = precision.max(1);
    // `%.{precision-1}e` gives exactly the significant digits `%g` keeps.
    let scientific = format!("{:.*e}", precision - 1, value);
    let (mantissa, exponent) = scientific
        .split_once('e')
        .expect("Rust's LowerExp always writes an exponent");
    let exponent: i32 = exponent.parse().expect("an integer exponent");
    let negative = mantissa.starts_with('-');
    let digits: String = mantissa
        .trim_start_matches('-')
        .chars()
        .filter(|character| *character != '.')
        .collect();
    let digits = digits.trim_end_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };

    let mut out = String::new();
    if negative {
        out.push('-');
    }
    if exponent < -4 || exponent >= precision as i32 {
        let (first, rest) = digits.split_at(1);
        out.push_str(first);
        if !rest.is_empty() {
            out.push('.');
            out.push_str(rest);
        }
        out.push('e');
        out.push_str(if exponent < 0 { "-" } else { "+" });
        out.push_str(&format!("{:02}", exponent.abs()));
    } else if exponent >= 0 {
        let integer_digits = exponent as usize + 1;
        if digits.len() <= integer_digits {
            out.push_str(digits);
            for _ in digits.len()..integer_digits {
                out.push('0');
            }
        } else {
            let (integer, fraction) = digits.split_at(integer_digits);
            out.push_str(integer);
            out.push('.');
            out.push_str(fraction);
        }
    } else {
        out.push_str("0.");
        for _ in 0..(-exponent - 1) {
            out.push('0');
        }
        out.push_str(digits);
    }
    out
}

/// A minimal ZIP writer: deflated entries in the given order, DOS times from
/// `now`. Both openpyxl's archive and `export_task.create_zip_file` are written
/// this way; the compressed bytes need not match CPython's zlib, only the
/// decompressed entries and their order do.
#[must_use]
pub fn zip(entries: &[(&str, Vec<u8>)], now: DateTime<Utc>) -> Vec<u8> {
    let (dos_time, dos_date) = dos_timestamp(now);
    let mut out = Vec::with_capacity(entries.iter().map(|(_, data)| data.len() + 128).sum());
    let mut central = Vec::new();
    let mut offsets = Vec::with_capacity(entries.len());
    let compressed: Vec<Vec<u8>> = entries.iter().map(|(_, data)| deflate(data)).collect();

    for ((name, data), deflated) in entries.iter().zip(&compressed) {
        offsets.push((out.len() as u32, crc32(data)));
        // Local file header (`PK\x03\x04`).
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes()); // version needed
        out.extend_from_slice(&flags(name).to_le_bytes());
        out.extend_from_slice(&8u16.to_le_bytes()); // deflate
        out.extend_from_slice(&dos_time.to_le_bytes());
        out.extend_from_slice(&dos_date.to_le_bytes());
        out.extend_from_slice(&crc32(data).to_le_bytes());
        out.extend_from_slice(&(deflated.len() as u32).to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // extra length
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(deflated);
    }

    let central_start = out.len() as u32;
    for (((name, data), deflated), (offset, crc)) in entries.iter().zip(&compressed).zip(&offsets) {
        // Central directory header (`PK\x01\x02`).
        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes()); // version made by
        central.extend_from_slice(&20u16.to_le_bytes()); // version needed
        central.extend_from_slice(&flags(name).to_le_bytes());
        central.extend_from_slice(&8u16.to_le_bytes());
        central.extend_from_slice(&dos_time.to_le_bytes());
        central.extend_from_slice(&dos_date.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&(deflated.len() as u32).to_le_bytes());
        central.extend_from_slice(&(data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(name.len() as u16).to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes()); // extra
        central.extend_from_slice(&0u16.to_le_bytes()); // comment
        central.extend_from_slice(&0u16.to_le_bytes()); // disk
        central.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
        central.extend_from_slice(&0u32.to_le_bytes()); // external attrs
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
    }
    let central_size = central.len() as u32;
    out.extend_from_slice(&central);
    // End of central directory (`PK\x05\x06`).
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&central_size.to_le_bytes());
    out.extend_from_slice(&central_start.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

fn flags(name: &str) -> u16 {
    if name.is_ascii() { 0 } else { 0x0800 }
}

fn deflate(data: &[u8]) -> Vec<u8> {
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
    // Writing into a `Vec` cannot fail; an error would mean the allocator is
    // gone, and openpyxl has no equivalent failure to reproduce.
    let _ = encoder.write_all(data);
    encoder.finish().unwrap_or_default()
}

fn crc32(data: &[u8]) -> u32 {
    // IEEE 802.3 CRC-32, the polynomial ZIP uses.
    let mut crc = 0xFFFF_FFFFu32;
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

/// `time.localtime()[:6]` in ZIP's DOS encoding (two-second resolution).
fn dos_timestamp(now: DateTime<Utc>) -> (u16, u16) {
    let year = now.year().clamp(1980, 2107) as u16;
    let month = now.month() as u16;
    let day = now.day() as u16;
    let hour = now.hour() as u16;
    let minute = now.minute() as u16;
    let second = now.second() as u16;
    let time = (hour << 11) | (minute << 5) | (second / 2);
    let date = ((year - 1980) << 9) | (month << 5) | day;
    (time, date)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_and_dimensions_follow_excel() {
        assert_eq!(column_name(0), "A");
        assert_eq!(column_name(25), "Z");
        assert_eq!(column_name(26), "AA");
        assert_eq!(column_name(701), "ZZ");
        assert_eq!(column_name(702), "AAA");
        assert_eq!(dimension(&[]), "A1:A1");
        assert_eq!(
            dimension(&[
                vec![Cell::Text("a".into())],
                vec![Cell::Text("b".into()); 3]
            ]),
            "A1:C2"
        );
    }

    #[test]
    fn text_is_escaped_like_lxml() {
        let rows = vec![vec![Cell::Text("a&<b>\"c' héllo 🚀  leading ".into())]];
        let xml = sheet_xml(&rows).expect("sheet");
        assert!(xml.contains(
            "<is><t xml:space=\"preserve\">a&amp;&lt;b&gt;\"c' h&#233;llo &#128640;  leading </t></is>"
        ));
        // Whitespace-only text also keeps the attribute: openpyxl compares
        // `value != value.strip()`, not the non-empty stripped text.
        let xml = sheet_xml(&[vec![Cell::Text("  ".into())]]).expect("sheet");
        assert!(xml.contains("<t xml:space=\"preserve\">  </t>"));
    }

    #[test]
    fn empty_error_and_formula_cells_match_openpyxl() {
        assert!(
            sheet_xml(&[vec![Cell::Text(String::new())]])
                .expect("sheet")
                .contains("<c r=\"A1\" t=\"inlineStr\"></c>")
        );
        assert!(
            sheet_xml(&[vec![Cell::Text("#N/A".into())]])
                .expect("sheet")
                .contains("<c r=\"A1\" t=\"e\"><v>#N/A</v></c>")
        );
        assert!(
            sheet_xml(&[vec![Cell::Text("=1+1".into())]])
                .expect("sheet")
                .contains("<c r=\"A1\"><f>1+1</f><v></v></c>")
        );
    }

    #[test]
    fn illegal_characters_are_rejected_with_python_message() {
        let error = sheet_xml(&[vec![Cell::Text("a\u{1}b".into())]]).expect_err("illegal");
        assert_eq!(error.to_string(), "a\u{1}b cannot be used in worksheets.");
        // Tab, newline and carriage return are legal in openpyxl.
        assert!(sheet_xml(&[vec![Cell::Text("a\tb\nc\rd".into())]]).is_ok());
    }

    #[test]
    fn long_strings_are_truncated_at_32767_code_points() {
        let text = "é".repeat(40_000);
        let xml = sheet_xml(&[vec![Cell::Text(text)]]).expect("sheet");
        // Each code point becomes one six-byte reference (`&#233;`), so the
        // count proves the truncation happened before escaping.
        assert_eq!(xml.matches("&#233;").count(), MAX_CELL_CHARS);
    }

    #[test]
    fn numbers_use_percent_g() {
        assert_eq!(percent_g(3.5, 16), "3.5");
        assert_eq!(percent_g(3.0, 16), "3");
        assert_eq!(percent_g(-0.0, 16), "-0");
        assert_eq!(percent_g(1e16, 16), "1e+16");
        assert_eq!(percent_g(1e-5, 16), "1e-05");
        assert_eq!(percent_g(0.0001, 16), "0.0001");
        assert_eq!(percent_g(f64::MAX, 16), "1.797693134862316e+308");
    }
}
