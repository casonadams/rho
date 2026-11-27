//! Per-turn telemetry row and cache-miss marker rendering.

use chrono::{DateTime, Local};
use rho_harness_core::config::DisplayUiConfig;
use rho_harness_core::presentation::TurnTelemetry;

use crate::ui::interactive::footer::{fit_right_aligned, format_tokens, truncate_with_ellipsis};
use crate::ui::theme::SGR_RESET;

fn format_local_timestamp(timestamp_unix_ms: u64) -> Option<String> {
    if timestamp_unix_ms == 0 {
        return None;
    }
    let millis = i64::try_from(timestamp_unix_ms).ok()?;
    let dt = DateTime::from_timestamp_millis(millis)?;
    Some(dt.with_timezone(&Local).format("%Y-%m-%d %H:%M:%S").to_string())
}

fn format_duration_ms(ms: u64) -> String {
    if ms < 1_000 {
        return format!("{ms}ms");
    }
    let tenths = (ms + 50) / 100;
    if tenths < 600 {
        format!("{}.{}s", tenths / 10, tenths % 10)
    } else {
        let secs = (ms + 500) / 1000;
        format!("{}m{:02}s", secs / 60, secs % 60)
    }
}

fn build_left_items(telemetry: &TurnTelemetry, display: &DisplayUiConfig) -> Vec<String> {
    let mut items = Vec::with_capacity(6);
    if let Some(ts) = format_local_timestamp(telemetry.timestamp_unix_ms) {
        items.push(ts);
    }
    if display.show_turn_time.unwrap_or(false) && telemetry.turn_elapsed_ms > 0 {
        items.push(format!("Δ {}", format_duration_ms(telemetry.turn_elapsed_ms)));
    }
    let input_total = telemetry.input_tokens.saturating_add(telemetry.cache_write_tokens);
    items.push(format!("↑{}", format_tokens(input_total)));
    items.push(format!("↓{}", format_tokens(telemetry.output_tokens)));
    if telemetry.cache_read_tokens > 0 {
        items.push(format!("R{}", format_tokens(telemetry.cache_read_tokens)));
    }
    if let Some(ttft) = telemetry.ttft_ms
        && ttft > 0
    {
        items.push(format!("⏱ {}", format_duration_ms(ttft)));
    }
    items
}

pub fn format_turn_stats_row(telemetry: &TurnTelemetry, display: &DisplayUiConfig, width: usize) -> Option<String> {
    if !display.show_token_usage.unwrap_or(false) {
        return None;
    }
    let left = build_left_items(telemetry, display).join("  ");
    let right = telemetry
        .tokens_per_second
        .filter(|&tps| tps > 0.0)
        .map(|tps| format!("{tps:.1} tok/s"))
        .unwrap_or_default();
    let inner_width = width.saturating_sub(2);
    let body = if right.is_empty() {
        truncate_with_ellipsis(&left, inner_width)
    } else {
        fit_right_aligned(&left, &right, inner_width)
    };
    Some(format!("\x1b[2m {body}{SGR_RESET}"))
}

pub fn format_cache_miss_marker(telemetry: &TurnTelemetry, display: &DisplayUiConfig, width: usize) -> Option<String> {
    if !display.cache_miss_marker.unwrap_or(false) {
        return None;
    }
    let tokens = telemetry.cache_miss_tokens?;
    let msg = format!("⊘ cache miss · {} tokens", format_tokens(tokens));
    let msg_cols = msg.chars().count();
    let available = width.saturating_sub(msg_cols);
    let body = if available >= 2 {
        let dashes = (available - 1).min(10);
        format!("{} {msg}", "─".repeat(dashes))
    } else {
        truncate_with_ellipsis(&msg, width)
    };
    Some(format!("\x1b[2m{body}{SGR_RESET}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::interactive::{InteractiveUi, TranscriptItem, UiEvent};
    use crate::ui::render::TerminalRenderer;

    fn sample_telemetry() -> TurnTelemetry {
        TurnTelemetry {
            timestamp_unix_ms: 1_700_000_000_000,
            turn_elapsed_ms: 16_500,
            request_duration_ms: 5_000,
            ttft_ms: Some(1_400),
            input_tokens: 1_200,
            output_tokens: 350,
            cache_read_tokens: 40_000,
            cache_write_tokens: 300,
            tokens_per_second: Some(70.0),
            cache_miss_tokens: Some(50_900),
        }
    }

    #[test]
    fn default_config_returns_none() {
        let telemetry = sample_telemetry();
        let display = DisplayUiConfig::default();
        assert!(format_turn_stats_row(&telemetry, &display, 100).is_none());

        let disabled = DisplayUiConfig {
            show_token_usage: Some(false),
            ..Default::default()
        };
        assert!(format_turn_stats_row(&telemetry, &disabled, 100).is_none());
    }

    #[test]
    fn full_row_includes_all_fields_and_right_aligned_tps() {
        let telemetry = sample_telemetry();
        let display = DisplayUiConfig {
            show_token_usage: Some(true),
            show_turn_time: Some(true),
            cache_miss_marker: None,
        };
        let row = format_turn_stats_row(&telemetry, &display, 100).expect("row should render");
        assert!(row.starts_with("\x1b[2m "));
        assert!(row.ends_with("\x1b[0m"));

        let expected_ts = format_local_timestamp(telemetry.timestamp_unix_ms).expect("valid timestamp");
        assert!(row.contains(&expected_ts));
        assert!(row.contains("Δ 16.5s"));
        assert!(row.contains("↑1.5k"));
        assert!(row.contains("↓350"));
        assert!(row.contains("R40.0k"));
        assert!(row.contains("⏱ 1.4s"));
        assert!(row.contains("70.0 tok/s"));
    }

    #[test]
    fn turn_time_false_omits_elapsed() {
        let mut telemetry = sample_telemetry();
        telemetry.turn_elapsed_ms = 72_000;
        let display = DisplayUiConfig {
            show_token_usage: Some(true),
            show_turn_time: Some(false),
            cache_miss_marker: None,
        };
        let row = format_turn_stats_row(&telemetry, &display, 100).expect("row should render");
        assert!(!row.contains("Δ "));
        assert!(row.contains("↑1.5k"));
        assert!(row.contains("↓350"));

        let with_time = DisplayUiConfig {
            show_turn_time: Some(true),
            ..display
        };
        let row_with_time = format_turn_stats_row(&telemetry, &with_time, 100).expect("row should render");
        assert!(row_with_time.contains("Δ 1m12s"));
    }

    #[test]
    fn cache_miss_marker_respects_config() {
        let telemetry = sample_telemetry();
        let enabled = DisplayUiConfig {
            cache_miss_marker: Some(true),
            ..Default::default()
        };
        let marker = format_cache_miss_marker(&telemetry, &enabled, 100).expect("marker should render");
        assert_eq!(marker, "\x1b[2m────────── ⊘ cache miss · 50.9k tokens\x1b[0m");

        let narrow = format_cache_miss_marker(&telemetry, &enabled, 30).expect("narrow marker should render");
        assert_eq!(narrow, "\x1b[2m── ⊘ cache miss · 50.9k tokens\x1b[0m");

        let tiny = format_cache_miss_marker(&telemetry, &enabled, 16).expect("tiny marker should render");
        assert!(crate::ui::block::visible_width(&tiny) <= 16, "got: {tiny:?}");
        assert!(tiny.contains("...") || tiny.contains('…'), "got: {tiny:?}");

        let disabled = DisplayUiConfig {
            cache_miss_marker: Some(false),
            ..Default::default()
        };
        assert!(format_cache_miss_marker(&telemetry, &disabled, 100).is_none());
        assert!(format_cache_miss_marker(&telemetry, &DisplayUiConfig::default(), 100).is_none());
    }

    #[test]
    fn renderer_emits_nothing_under_default_config_and_emits_when_enabled() {
        let (ui, mut events) = InteractiveUi::channel();
        let mut renderer = TerminalRenderer::with_ui(ui);
        renderer.set_width(100);

        let telemetry = sample_telemetry();
        renderer.print_turn_telemetry(&telemetry);
        assert!(events.try_recv().is_err());

        renderer.set_display_config(DisplayUiConfig {
            show_token_usage: Some(true),
            show_turn_time: Some(true),
            cache_miss_marker: Some(true),
        });
        renderer.print_turn_telemetry(&telemetry);

        let Ok(UiEvent::Transcript(TranscriptItem::Notice(row))) = events.try_recv() else {
            panic!("expected turn stats notice");
        };
        assert!(row.contains("↑1.5k"));
        assert!(row.contains("70.0 tok/s"));

        let Ok(UiEvent::Transcript(TranscriptItem::Notice(marker))) = events.try_recv() else {
            panic!("expected cache miss marker notice");
        };
        assert!(marker.contains("────────── ⊘ cache miss · 50.9k tokens"));
        assert!(events.try_recv().is_err());
    }

    #[test]
    fn format_duration_ms_handles_subsecond_and_rounding_boundaries() {
        assert_eq!(format_duration_ms(40), "40ms");
        assert_eq!(format_duration_ms(999), "999ms");
        assert_eq!(format_duration_ms(1_400), "1.4s");
        assert_eq!(format_duration_ms(59_949), "59.9s");
        assert_eq!(format_duration_ms(59_999), "1m00s");
        assert_eq!(format_duration_ms(72_000), "1m12s");
    }
}
