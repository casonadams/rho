use rho_harness_core::error::{AppError, Result};
use std::io::Read;

const MAX_ROWS: usize = 50;
const MAX_COLS: usize = 20;
const MAX_CELL_CHARS: usize = 200;

pub fn extract_json(raw: &str) -> String {
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(raw) {
        serde_json::to_string_pretty(&val).unwrap_or_else(|_| raw.to_string())
    } else {
        raw.to_string()
    }
}

fn sanitize_cell(cell: &str) -> String {
    let flattened = cell.replace(['\r', '\n'], " ");
    let escaped = flattened.replace('|', "\\|");
    if escaped.chars().count() > MAX_CELL_CHARS {
        let truncated: String = escaped.chars().take(MAX_CELL_CHARS).collect();
        format!("{truncated}...")
    } else {
        escaped
    }
}

fn parse_csv_rows(raw: &str, delimiter: u8) -> Vec<Vec<String>> {
    let mut rdr = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .flexible(true)
        .has_headers(false)
        .from_reader(raw.as_bytes());

    let mut rows = Vec::new();
    for record in rdr.records().flatten() {
        let row: Vec<String> = record.iter().map(sanitize_cell).collect();
        if row.iter().any(|c| !c.trim().is_empty()) {
            rows.push(row);
        }
    }
    rows
}

fn pad_row(mut row: Vec<String>, col_count: usize) -> Vec<String> {
    row.truncate(col_count);
    while row.len() < col_count {
        row.push(String::new());
    }
    row
}

fn format_notes(total_rows: usize, shown_rows: usize, orig_cols: usize, col_count: usize) -> Option<String> {
    let mut notes = Vec::new();
    if total_rows > shown_rows {
        notes.push(format!("showing first {shown_rows} of {total_rows} data rows"));
    }
    if orig_cols > col_count {
        notes.push(format!("showing first {col_count} of {orig_cols} columns"));
    }
    (!notes.is_empty()).then(|| format!("\n[Truncated: {}]", notes.join("; ")))
}

pub fn extract_csv(raw: &str, delimiter: u8) -> String {
    let mut all_rows = parse_csv_rows(raw, delimiter);
    if all_rows.is_empty() {
        return "Empty CSV/TSV content.".to_string();
    }

    let orig_cols = all_rows.iter().map(Vec::len).max().unwrap_or(0);
    let col_count = orig_cols.min(MAX_COLS);
    let header = pad_row(all_rows.remove(0), col_count);
    let total_data = all_rows.len();
    let data_limit = (MAX_ROWS - 1).min(total_data);

    let mut out = format!("| {} |\n", header.join(" | "));
    let sep: Vec<&str> = vec!["---"; col_count];
    out.push_str(&format!("| {} |\n", sep.join(" | ")));

    for row in all_rows.into_iter().take(data_limit) {
        let padded = pad_row(row, col_count);
        out.push_str(&format!("| {} |\n", padded.join(" | ")));
    }

    if let Some(notes) = format_notes(total_data, data_limit, orig_cols, col_count) {
        out.push_str(&notes);
    }
    out.trim_end().to_string()
}

const MAX_PDF_STREAM_BYTES: u64 = 64 * 1024 * 1024;
const MAX_PDF_TOTAL_INFLATED_BYTES: u64 = 512 * 1024 * 1024;

fn is_zlib_header(pair: &[u8]) -> bool {
    pair[0] & 0x0f == 8 && pair[0] >> 4 <= 7 && u16::from_be_bytes([pair[0], pair[1]]).is_multiple_of(31)
}

fn inflated_len(data: &[u8], limit: u64) -> u64 {
    let mut decoder = flate2::read::ZlibDecoder::new(data);
    let mut buf = [0u8; 64 * 1024];
    let mut total = 0;
    while total <= limit {
        match decoder.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => total += n as u64,
        }
    }
    total
}

/// pdf-extract (lopdf 0.42) inflates streams without an output cap, so a small file can exhaust memory.
/// Every zlib-looking offset is trial-inflated under a budget, which holds regardless of how streams are delimited.
fn ensure_bounded_inflation(bytes: &[u8], stream_cap: u64, total_cap: u64) -> Result<()> {
    let mut total = 0;
    for (offset, pair) in bytes.windows(2).enumerate() {
        if !is_zlib_header(pair) {
            continue;
        }
        let limit = stream_cap.min(total_cap - total);
        let inflated = inflated_len(&bytes[offset..], limit);
        if inflated > limit {
            return Err(AppError::Tool(format!(
                "PDF rejected: compressed data expands beyond the safe limit ({} MiB per stream, {} MiB total)",
                stream_cap >> 20,
                total_cap >> 20
            )));
        }
        total += inflated;
    }
    Ok(())
}

pub async fn extract_pdf_bytes(bytes: Vec<u8>) -> Result<String> {
    tokio::task::spawn_blocking(move || {
        ensure_bounded_inflation(&bytes, MAX_PDF_STREAM_BYTES, MAX_PDF_TOTAL_INFLATED_BYTES)?;
        pdf_extract::extract_text_from_mem(&bytes).map_err(|e| AppError::Tool(format!("PDF extraction error: {e}")))
    })
    .await
    .map_err(|e| AppError::Tool(format!("Tokio spawn error during PDF extraction: {e}")))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{Compression, write::ZlibEncoder};
    use std::io::Write;

    fn zlib_of_zeros(len: usize) -> Vec<u8> {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(&vec![0u8; len]).unwrap();
        encoder.finish().unwrap()
    }

    fn pdf_with(stream: &[u8]) -> Vec<u8> {
        let mut pdf = b"%PDF-1.4\n1 0 obj\n<< /Filter /FlateDecode >>\nstream\n".to_vec();
        pdf.extend_from_slice(stream);
        pdf.extend_from_slice(b"\nendstream\nendobj\n");
        pdf
    }

    #[test]
    fn a_stream_expanding_past_the_cap_is_rejected() {
        let pdf = pdf_with(&zlib_of_zeros(2 << 20));
        let err = ensure_bounded_inflation(&pdf, 1 << 20, 8 << 20).unwrap_err();
        assert!(err.to_string().contains("expands beyond the safe limit"), "{err}");
    }

    #[test]
    fn many_streams_exceeding_the_total_budget_are_rejected() {
        let stream = zlib_of_zeros(600 << 10);
        let mut pdf = pdf_with(&stream);
        pdf.extend(pdf_with(&stream));
        pdf.extend(pdf_with(&stream));
        assert!(ensure_bounded_inflation(&pdf, 1 << 20, 1 << 20).is_err());
        assert!(ensure_bounded_inflation(&pdf, 1 << 20, 4 << 20).is_ok());
    }

    #[test]
    fn modest_streams_and_non_zlib_bytes_pass() {
        let pdf = pdf_with(&zlib_of_zeros(100 << 10));
        assert!(ensure_bounded_inflation(&pdf, 1 << 20, 8 << 20).is_ok());
        assert!(ensure_bounded_inflation(b"corrupt-pdf-bytes", 1 << 20, 8 << 20).is_ok());
    }

    #[tokio::test]
    async fn extraction_refuses_a_decompression_bomb_before_parsing() {
        let pdf = pdf_with(&zlib_of_zeros((MAX_PDF_STREAM_BYTES as usize) + 1));
        let err = extract_pdf_bytes(pdf).await.unwrap_err();
        assert!(err.to_string().contains("expands beyond the safe limit"), "{err}");
    }
}
