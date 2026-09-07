use rho_harness_core::error::{AppError, Result};

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

pub async fn extract_pdf_bytes(bytes: Vec<u8>) -> Result<String> {
    tokio::task::spawn_blocking(move || {
        pdf_extract::extract_text_from_mem(&bytes).map_err(|e| AppError::Tool(format!("PDF extraction error: {e}")))
    })
    .await
    .map_err(|e| AppError::Tool(format!("Tokio spawn error during PDF extraction: {e}")))?
}
