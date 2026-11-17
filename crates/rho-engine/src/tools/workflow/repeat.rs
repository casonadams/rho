use super::executor::{Executor, Frame, RepeatPlan, StepOutcome, fingerprints, frame_scope, render_value};
use rho_harness_core::workflow::template::is_truthy;
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StopReason {
    Clean,
    NoProgress,
    MaxRounds,
}

impl StopReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::NoProgress => "no progress",
            Self::MaxRounds => "max rounds",
        }
    }
}

pub(crate) fn stop_reason(
    remaining: &Value,
    current: &BTreeSet<String>,
    previous: Option<&BTreeSet<String>>,
    round: usize,
    max: usize,
    stop_on_no_progress: bool,
) -> Option<StopReason> {
    if !is_truthy(remaining) {
        Some(StopReason::Clean)
    } else if stop_on_no_progress && previous == Some(current) {
        Some(StopReason::NoProgress)
    } else if round >= max {
        Some(StopReason::MaxRounds)
    } else {
        None
    }
}

fn parse_max(template: &str, scope: &Value) -> Result<usize, String> {
    let value = render_value(template, scope)?;
    let parsed = match &value {
        Value::Number(n) => n.as_u64().and_then(|n| usize::try_from(n).ok()),
        Value::String(s) => s.trim().parse::<usize>().ok(),
        _ => None,
    };
    parsed
        .filter(|n| *n > 0)
        .ok_or_else(|| format!("repeat.max must be a positive integer (got {value})"))
}

impl Executor<'_> {
    pub(crate) async fn run_repeat(&self, plan: RepeatPlan<'_>) -> Result<StepOutcome, String> {
        let Some(repeat) = &plan.call.repeat else {
            return Err("internal error: repeat plan without repeat settings".to_string());
        };
        let max = parse_max(&repeat.max, plan.scope)?;
        let mut carried = Map::new();
        let mut previous: Option<BTreeSet<String>> = None;
        for round in 1..=max {
            let mut provided = plan.provided.clone();
            provided.extend(carried.clone());
            let inputs = plan.target.resolve_inputs(provided)?;
            let frame = Frame {
                bundle: plan.bundle,
                workflow: plan.target,
                path: format!("{} › round {round}", plan.path),
                scope: frame_scope(inputs, round),
            };
            let outputs = self.run_workflow(frame).await?;
            for name in &plan.call.carry {
                carried.insert(name.clone(), outputs.get(name).cloned().unwrap_or(Value::Null));
            }
            let remaining = outputs.get(&repeat.until_empty).cloned().unwrap_or(Value::Null);
            let current = fingerprints(&remaining);
            let stop = stop_reason(
                &remaining,
                &current,
                previous.as_ref(),
                round,
                max,
                repeat.stop_on_no_progress,
            );
            self.log.event(
                "round_finished",
                json!({ "step": plan.path, "round": round, "remaining": current.len(), "stop": stop.map(StopReason::as_str) }),
            );
            if let Some(reason) = stop {
                let mut result = outputs.as_object().cloned().unwrap_or_default();
                result.insert("rounds".to_string(), json!(round));
                result.insert("stop_reason".to_string(), json!(reason.as_str()));
                let detail = format!("{round} round(s), stopped: {}", reason.as_str());
                return Ok(StepOutcome::done(Value::Object(result), detail));
            }
            previous = Some(current);
        }
        Err(format!("repeat ended without a stop reason after {max} rounds"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn stop_reasons_follow_priority_clean_then_no_progress_then_max() {
        let found = json!([{"fingerprint": "a"}]);
        let a = set(&["a"]);
        let cases = [
            (json!([]), None, 1, 6, true, Some(StopReason::Clean)),
            (
                found.clone(),
                Some(set(&["a"])),
                2,
                6,
                true,
                Some(StopReason::NoProgress),
            ),
            (found.clone(), Some(set(&["a"])), 2, 6, false, None),
            (
                found.clone(),
                Some(set(&["b"])),
                6,
                6,
                true,
                Some(StopReason::MaxRounds),
            ),
            (found.clone(), None, 1, 6, true, None),
        ];
        for (remaining, previous, round, max, no_progress, expected) in cases {
            let got = stop_reason(&remaining, &a, previous.as_ref(), round, max, no_progress);
            assert_eq!(got, expected, "round {round} previous {previous:?}");
        }
    }

    #[test]
    fn max_accepts_numbers_and_numeric_strings_only() {
        let scope = json!({"inputs": {"n": 4, "s": "3", "bad": "x", "zero": 0}});
        assert_eq!(parse_max("{{inputs.n}}", &scope).unwrap(), 4);
        assert_eq!(parse_max("{{inputs.s}}", &scope).unwrap(), 3);
        assert_eq!(parse_max("5", &scope).unwrap(), 5);
        assert!(parse_max("{{inputs.bad}}", &scope).is_err());
        assert!(parse_max("{{inputs.zero}}", &scope).is_err());
    }
}
