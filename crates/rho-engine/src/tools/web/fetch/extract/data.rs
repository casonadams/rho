use rho_harness_core::error::{AppError, Result};

pub fn extract_json(raw: &str) -> String {
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(raw) {
        serde_json::to_string_pretty(&val).unwrap_or_else(|_| raw.to_string())
    } else {
        raw.to_string()
    }
}

pub fn extract_csv(raw: &str, delimiter: u8) -> String {
    let mut rdr = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .flexible(true)
        .from_reader(raw.as_bytes());

    let mut out = String::new();
    if let Ok(headers) = rdr.headers() {
        let header_row = headers.iter().collect::<Vec<_>>().join(" | ");
        out.push_str(&format!("| {header_row} |\n"));
        let sep = headers.iter().map(|_| "---").collect::<Vec<_>>().join(" | ");
        out.push_str(&format!("| {sep} |\n"));
    }

    for record in rdr.records().take(100).flatten() {
        let row = record.iter().collect::<Vec<_>>().join(" | ");
        out.push_str(&format!("| {row} |\n"));
    }
    out
}

pub async fn extract_pdf_bytes(bytes: Vec<u8>) -> Result<String> {
    tokio::task::spawn_blocking(move || {
        pdf_extract::extract_text_from_mem(&bytes).map_err(|e| AppError::Tool(format!("PDF extraction error: {e}")))
    })
    .await
    .map_err(|e| AppError::Tool(format!("Tokio spawn error during PDF extraction: {e}")))?
}
