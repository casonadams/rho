use super::{PreparedQuestion, RECOMMENDED_SUFFIX};
use rho_harness_core::a2ui::A2uiModalResult;
use serde::Serialize;
use serde_json::Value;

const OTHER_SENTINEL: &str = "__other__";

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum AskOutcome {
    Submitted,
    Cancelled,
    TimedOut,
    Undelivered,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum AnswerStatus {
    Answered,
    Skipped,
    Unanswered,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct AskOutput {
    outcome: AskOutcome,
    answers: Vec<AskAnswerOutput>,
}

#[derive(Debug, Clone, Serialize)]
struct AskAnswerOutput {
    id: String,
    question: String,
    status: AnswerStatus,
    selected_index: Option<usize>,
    selected_indices: Vec<usize>,
    user_response: String,
}

impl AskAnswerOutput {
    fn new(q: &PreparedQuestion, status: AnswerStatus, selected_indices: Vec<usize>, user_response: String) -> Self {
        Self {
            id: q.id.clone(),
            question: q.raw_question.clone(),
            status,
            selected_index: selected_indices.first().copied(),
            selected_indices,
            user_response,
        }
    }

    fn answered(q: &PreparedQuestion, selected_indices: Vec<usize>, user_response: String) -> Self {
        Self::new(q, AnswerStatus::Answered, selected_indices, user_response)
    }

    fn skipped(q: &PreparedQuestion) -> Self {
        Self::new(q, AnswerStatus::Skipped, Vec::new(), String::new())
    }

    fn unanswered(q: &PreparedQuestion) -> Self {
        Self::new(q, AnswerStatus::Unanswered, Vec::new(), String::new())
    }
}

pub(super) fn build_output(questions: &[PreparedQuestion], result: Option<A2uiModalResult>) -> AskOutput {
    match result {
        None => unanswered_output(AskOutcome::Undelivered, questions),
        Some(A2uiModalResult::Cancelled) => unanswered_output(AskOutcome::Cancelled, questions),
        Some(A2uiModalResult::TimedOut) => unanswered_output(AskOutcome::TimedOut, questions),
        Some(A2uiModalResult::Submitted { data_model, .. }) => AskOutput {
            outcome: AskOutcome::Submitted,
            answers: questions
                .iter()
                .map(|q| extract_question_answer(q, &data_model))
                .collect(),
        },
    }
}

fn unanswered_output(outcome: AskOutcome, questions: &[PreparedQuestion]) -> AskOutput {
    AskOutput {
        outcome,
        answers: questions.iter().map(AskAnswerOutput::unanswered).collect(),
    }
}

fn extract_question_answer(q: &PreparedQuestion, data_model: &Value) -> AskAnswerOutput {
    let answer_val = data_model.pointer(&format!("/answers/{}", q.key));
    let other_text = data_model
        .pointer(&format!("/answers/{}_other", q.key))
        .and_then(Value::as_str)
        .map(str::trim)
        .map(strip_recommended_suffix)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    if q.choices.is_empty() {
        return extract_free_text_answer(q, answer_val);
    }
    if q.multi_select {
        return extract_multi_select_answer(q, answer_val, other_text);
    }
    extract_single_select_answer(q, answer_val, other_text)
}

fn extract_free_text_answer(q: &PreparedQuestion, answer_val: Option<&Value>) -> AskAnswerOutput {
    let text = answer_val
        .and_then(Value::as_str)
        .map(str::trim)
        .map(strip_recommended_suffix)
        .unwrap_or("")
        .to_string();
    if text.is_empty() {
        return AskAnswerOutput::skipped(q);
    }
    AskAnswerOutput::answered(q, Vec::new(), text)
}

fn extract_single_select_answer(
    q: &PreparedQuestion,
    answer_val: Option<&Value>,
    other_text: Option<String>,
) -> AskAnswerOutput {
    if is_other_selection(answer_val) {
        return custom_text_answer(q, other_text.unwrap_or_default());
    }
    if let Some(idx) = answer_val.and_then(|v| resolve_choice_index(q, v)) {
        let raw = strip_recommended_suffix(&q.raw_labels[idx]).to_string();
        return AskAnswerOutput::answered(q, vec![idx], raw);
    }
    if let Some(text) = other_text {
        return custom_text_answer(q, text);
    }
    if q.allow_other
        && let Some(raw) = answer_val
            .and_then(Value::as_str)
            .map(str::trim)
            .map(strip_recommended_suffix)
            .filter(|s| !s.is_empty())
    {
        return custom_text_answer(q, raw.to_string());
    }
    AskAnswerOutput::skipped(q)
}

fn extract_multi_select_answer(
    q: &PreparedQuestion,
    answer_val: Option<&Value>,
    other_text: Option<String>,
) -> AskAnswerOutput {
    let items: Vec<&Value> = match answer_val {
        Some(Value::Array(arr)) => arr.iter().collect(),
        Some(other) => vec![other],
        None => Vec::new(),
    };

    let mut selected_indices = Vec::new();
    let mut parts = Vec::new();
    let mut saw_other = false;

    for item in items {
        if is_other_selection(Some(item)) {
            saw_other = true;
            continue;
        }
        if let Some(idx) = resolve_choice_index(q, item)
            && !selected_indices.contains(&idx)
        {
            selected_indices.push(idx);
            parts.push(strip_recommended_suffix(&q.raw_labels[idx]).to_string());
        }
    }

    if (saw_other || q.allow_other)
        && let Some(other) = other_text
    {
        parts.push(other);
    }

    if parts.is_empty() {
        return AskAnswerOutput::skipped(q);
    }

    AskAnswerOutput::answered(q, selected_indices, parts.join(", "))
}

fn custom_text_answer(q: &PreparedQuestion, text: String) -> AskAnswerOutput {
    if text.is_empty() {
        return AskAnswerOutput::skipped(q);
    }
    AskAnswerOutput::answered(q, Vec::new(), text)
}

fn is_other_selection(value: Option<&Value>) -> bool {
    value
        .and_then(Value::as_str)
        .is_some_and(|s| s == OTHER_SENTINEL || s.eq_ignore_ascii_case("other"))
}

fn resolve_choice_index(q: &PreparedQuestion, value: &Value) -> Option<usize> {
    if let Some(idx) = value.as_u64().and_then(|n| usize::try_from(n).ok()) {
        return (idx < q.raw_labels.len()).then_some(idx);
    }
    let text = value.as_str()?.trim();
    if let Ok(idx) = text.parse::<usize>()
        && idx < q.raw_labels.len()
    {
        return Some(idx);
    }
    let clean = strip_recommended_suffix(text);
    q.choices
        .iter()
        .position(|c| c.display_label == text || c.deduped_label == clean)
}

fn strip_recommended_suffix(text: &str) -> &str {
    text.strip_suffix(RECOMMENDED_SUFFIX).unwrap_or(text)
}
