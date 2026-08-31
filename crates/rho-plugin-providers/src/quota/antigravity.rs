use super::types::{QuotaWindow, parse_reset_time};
use chrono::Utc;

pub fn parse_antigravity_usage(usage: &serde_json::Value, model_id: Option<&str>) -> Vec<QuotaWindow> {
    if let Some(models) = usage.get("models").and_then(|m| m.as_object()) {
        let normalized = model_id.map(|s| s.to_ascii_lowercase());
        let mut candidates: Vec<(&String, &serde_json::Value)> = Vec::new();

        if let Some(target) = &normalized {
            for (id, info) in models {
                let id_lower = id.to_ascii_lowercase();
                if (id_lower == *target || id_lower.starts_with(&format!("{target}-")))
                    && info.get("quotaInfo").is_some()
                {
                    candidates.push((id, info));
                }
            }
        }

        if candidates.is_empty() {
            for (id, info) in models {
                if info.get("quotaInfo").is_some() {
                    candidates.push((id, info));
                }
            }
        }

        let selected = candidates.iter().min_by(|a, b| {
            let frac_a =
                a.1.get("quotaInfo")
                    .and_then(|qi| qi.get("remainingFraction"))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(1.0);
            let frac_b =
                b.1.get("quotaInfo")
                    .and_then(|qi| qi.get("remainingFraction"))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(1.0);
            frac_a.partial_cmp(&frac_b).unwrap_or(std::cmp::Ordering::Equal)
        });

        if let Some((_, info)) = selected
            && let Some(qi) = info.get("quotaInfo")
        {
            let fraction = qi
                .get("remainingFraction")
                .or_else(|| qi.get("fraction"))
                .and_then(|v| v.as_f64());
            if let Some(frac) = fraction {
                let used_percent = ((1.0 - frac) * 100.0).clamp(0.0, 100.0);
                let resets_at = parse_reset_time(&qi.get("resetTime").cloned());
                let label = if let Some(reset) = resets_at {
                    let total_seconds = (reset - Utc::now()).num_seconds();
                    if total_seconds > 36 * 3600 { "7d" } else { "5h" }
                } else {
                    "pool"
                };
                return vec![QuotaWindow {
                    label: label.to_string(),
                    used_percent,
                    resets_at,
                    used_value: used_percent,
                    limit_value: 100.0,
                    is_currency: false,
                    limited: false,
                }];
            }
        }
    }

    let groups = usage
        .get("groups")
        .or_else(|| usage.get("userQuota").and_then(|u| u.get("groups")))
        .or_else(|| usage.get("quota").and_then(|q| q.get("groups")));

    let mut windows = Vec::new();

    if let Some(arr) = groups.and_then(|g| g.as_array()) {
        for group in arr {
            if let Some(buckets) = group.get("buckets").and_then(|b| b.as_array()) {
                for bucket in buckets {
                    let fraction = bucket
                        .get("remainingFraction")
                        .or_else(|| bucket.get("fraction"))
                        .and_then(|v| v.as_f64());
                    if let Some(frac) = fraction {
                        let used_percent = ((1.0 - frac) * 100.0).clamp(0.0, 100.0);
                        let label = bucket
                            .get("displayName")
                            .or_else(|| bucket.get("label"))
                            .or_else(|| bucket.get("window"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("pool");
                        windows.push(QuotaWindow {
                            label: label.to_string(),
                            used_percent,
                            resets_at: parse_reset_time(&bucket.get("resetTime").cloned()),
                            used_value: used_percent,
                            limit_value: 100.0,
                            is_currency: false,
                            limited: false,
                        });
                    }
                }
            }
        }
    } else if let Some(buckets) = usage.get("buckets").and_then(|b| b.as_array()) {
        for bucket in buckets {
            let fraction = bucket
                .get("remainingFraction")
                .or_else(|| bucket.get("fraction"))
                .and_then(|v| v.as_f64())
                .unwrap_or(1.0);
            let used_percent = ((1.0 - fraction) * 100.0).clamp(0.0, 100.0);
            let label = bucket
                .get("label")
                .or_else(|| bucket.get("displayName"))
                .and_then(|v| v.as_str())
                .unwrap_or("pool");
            windows.push(QuotaWindow {
                label: label.to_string(),
                used_percent,
                resets_at: parse_reset_time(&bucket.get("resetTime").cloned()),
                used_value: used_percent,
                limit_value: 100.0,
                is_currency: false,
                limited: false,
            });
        }
    }

    windows.sort_by_key(|w| {
        let l = w.label.to_lowercase();
        if l.contains("7d") || l.contains("week") {
            0
        } else if l.contains("5h") || l.contains("hour") {
            1
        } else {
            2
        }
    });

    windows
}
