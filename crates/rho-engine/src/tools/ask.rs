use crate::decision::{DecisionHandle, DecisionQuestion, DecisionRequest};
use crate::tools::engine_tool::EngineTool;
use crate::tools::types::{ToolResult, generated_schema, normalize_tool_arguments, try_parse_tool_args};
use rho_harness_core::a2ui::{
    A2uiComponent, A2uiComponentKind, A2uiMessage, A2uiModalRequest, A2uiModalResult, ActionBinding, BoundString,
    ButtonVariant, ChoiceOption, CreateSurface, RowJustify, SemanticTone, SurfacePlacement, TextUsageHint,
    UpdateComponents, UpdateDataModel,
};
use rho_harness_core::error::AppError;
use rho_harness_core::presentation::Presenter;
use rho_harness_core::session::redact_credentials_text;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

pub const MAX_QUESTIONS: usize = 5;
pub const MAX_CHOICES: usize = 4;
pub const ASK_SURFACE_ID: &str = "rho:ask";
const ASK_TOOL_NAME: &str = "ask";
const DEFAULT_DECISION_TIMEOUT: Duration = Duration::from_millis(600);
const RECOMMENDED_SUFFIX: &str = " (Recommended)";
const OTHER_SENTINEL: &str = "__other__";

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct AskArgs {
    /// 1 to 5 questions to present to the user in a structured modal.
    pub questions: Vec<AskQuestionArg>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct AskQuestionArg {
    /// Optional stable identifier for this question (defaults to "q1", "q2", ...).
    #[serde(default)]
    pub id: Option<String>,
    /// Non-empty question prompt shown to the user.
    pub question: String,
    /// Optional short section header for the question.
    #[serde(default)]
    pub header: Option<String>,
    /// Optional 0 to 4 structured choices. Omit or leave empty for a free-text input.
    #[serde(default)]
    pub choices: Option<Vec<AskChoiceArg>>,
    /// Allow selecting multiple choices (default false).
    #[serde(default)]
    pub multi_select: Option<bool>,
    /// Optional 0-based index of the recommended choice.
    #[serde(default)]
    pub recommended: Option<usize>,
    /// Allow entering a custom "Other" response when choices are provided (default true).
    #[serde(default)]
    pub allow_other: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct AskChoiceArg {
    /// Display label for the choice.
    pub label: String,
    /// Optional supporting description for the choice.
    #[serde(default)]
    pub description: Option<String>,
}

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
struct AskOutput {
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

#[derive(Debug, Clone)]
struct PreparedChoice {
    display_label: String,
    deduped_label: String,
    description: Option<String>,
}

#[derive(Debug, Clone)]
struct PreparedQuestion {
    id: String,
    raw_question: String,
    redacted_question: String,
    redacted_header: Option<String>,
    raw_labels: Vec<String>,
    choices: Vec<PreparedChoice>,
    multi_select: bool,
    allow_other: bool,
    recommended_index: Option<usize>,
}

pub struct AskTool {
    presenter: Arc<dyn Presenter>,
    decision: Option<DecisionHandle>,
    decision_timeout: Duration,
}

impl AskTool {
    pub fn new(presenter: Arc<dyn Presenter>) -> Self {
        Self {
            presenter,
            decision: None,
            decision_timeout: DEFAULT_DECISION_TIMEOUT,
        }
    }

    pub fn with_decision(mut self, decision: DecisionHandle) -> Self {
        self.decision = Some(decision);
        self
    }

    pub fn with_decision_timeout(mut self, timeout: Duration) -> Self {
        self.decision_timeout = timeout;
        self
    }

    async fn prepare_questions(&self, args: AskArgs) -> Result<Vec<PreparedQuestion>, AppError> {
        if args.questions.is_empty() {
            return Err(AppError::Tool("ask requires at least 1 question".to_string()));
        }
        if args.questions.len() > MAX_QUESTIONS {
            return Err(AppError::Tool(format!(
                "ask accepts at most {MAX_QUESTIONS} questions, got {}",
                args.questions.len()
            )));
        }

        let mut prepared = Vec::with_capacity(args.questions.len());
        for (idx, q) in args.questions.into_iter().enumerate() {
            prepared.push(self.prepare_question(idx, q).await?);
        }
        Ok(prepared)
    }

    async fn prepare_question(&self, idx: usize, q: AskQuestionArg) -> Result<PreparedQuestion, AppError> {
        let id =
            q.id.as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| format!("q{}", idx + 1));

        let raw_question = q.question.trim().to_string();
        if raw_question.is_empty() {
            return Err(AppError::Tool(format!("question '{id}' text must not be empty")));
        }

        let raw_choices = q.choices.unwrap_or_default();
        if raw_choices.len() > MAX_CHOICES {
            return Err(AppError::Tool(format!(
                "question '{id}' has {} choices, maximum is {MAX_CHOICES}",
                raw_choices.len()
            )));
        }

        if let Some(rec) = q.recommended
            && (raw_choices.is_empty() || rec >= raw_choices.len())
        {
            return Err(AppError::Tool(format!(
                "question '{id}' recommended index {rec} is out of range for {} choices",
                raw_choices.len()
            )));
        }

        let redacted_question = redact_credentials_text(&raw_question);
        let redacted_header = q
            .header
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(redact_credentials_text);

        let recommended_index = self
            .resolve_recommended_index(&raw_question, q.header.as_deref(), &raw_choices, q.recommended)
            .await;

        let raw_labels: Vec<String> = raw_choices.iter().map(|c| c.label.clone()).collect();
        let choices = prepare_display_choices(&raw_choices, recommended_index);
        let allow_other = if choices.is_empty() {
            false
        } else {
            q.allow_other.unwrap_or(true)
        };

        Ok(PreparedQuestion {
            id,
            raw_question,
            redacted_question,
            redacted_header,
            raw_labels,
            choices,
            multi_select: q.multi_select.unwrap_or(false),
            allow_other,
            recommended_index,
        })
    }

    async fn resolve_recommended_index(
        &self,
        question: &str,
        header: Option<&str>,
        choices: &[AskChoiceArg],
        explicit: Option<usize>,
    ) -> Option<usize> {
        if choices.is_empty() {
            return None;
        }
        if let Some(idx) = explicit {
            return Some(idx);
        }
        if choices.len() >= 2
            && let Some(decision) = &self.decision
        {
            return Some(recommend_with_decision(decision, self.decision_timeout, question, header, choices).await);
        }
        Some(0)
    }
}

#[async_trait::async_trait]
impl EngineTool for AskTool {
    fn name(&self) -> &str {
        ASK_TOOL_NAME
    }

    fn description(&self) -> &str {
        "Ask the user 1 to 5 clarifying questions with optional structured choices and receive their structured answers."
    }

    fn parameters(&self) -> Value {
        generated_schema::<AskArgs>()
    }

    async fn execute(&self, args: Value) -> Result<ToolResult, AppError> {
        let normalized = normalize_tool_arguments(args);
        let parsed: AskArgs =
            try_parse_tool_args(&normalized).map_err(|err| AppError::Tool(format!("invalid ask arguments: {err}")))?;
        let prepared = self.prepare_questions(parsed).await?;
        let modal = build_modal_request(&prepared);
        let result = self.presenter.request_a2ui_modal(modal).await;
        let output = build_output(&prepared, result);
        let encoded = serde_json::to_string_pretty(&output)
            .map_err(|err| AppError::Tool(format!("failed to serialize ask output: {err}")))?;
        Ok(ToolResult::success(encoded))
    }
}

async fn recommend_with_decision(
    decision: &DecisionHandle,
    timeout: Duration,
    question: &str,
    header: Option<&str>,
    choices: &[AskChoiceArg],
) -> usize {
    let mut criteria = BTreeMap::new();
    for (idx, choice) in choices.iter().enumerate() {
        let desc = match choice.description.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(d) => format!("{}: {d}", choice.label),
            None => choice.label.clone(),
        };
        criteria.insert(idx.to_string(), desc);
    }

    let input = match header.map(str::trim).filter(|s| !s.is_empty()) {
        Some(h) => format!("{h}\n{question}"),
        None => question.to_string(),
    };

    let mut request = DecisionRequest::new(input);
    request.questions.insert(
        "choice".to_string(),
        DecisionQuestion::Choice {
            instructions: "Select the most likely or recommended default choice index.".to_string(),
            criteria,
        },
    );

    match tokio::time::timeout(timeout, decision.decide(request)).await {
        Ok(Ok(response)) => response
            .answers
            .get("choice")
            .and_then(|answer| answer.as_choice())
            .and_then(|key| key.trim().parse::<usize>().ok())
            .filter(|&idx| idx < choices.len())
            .unwrap_or(0),
        _ => 0,
    }
}

fn prepare_display_choices(raw_choices: &[AskChoiceArg], recommended_index: Option<usize>) -> Vec<PreparedChoice> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut used: HashSet<String> = HashSet::new();
    let mut prepared = Vec::with_capacity(raw_choices.len());

    for (idx, choice) in raw_choices.iter().enumerate() {
        let base = redact_credentials_text(choice.label.trim());
        let deduped = disambiguate_label(&base, &mut counts, &mut used);
        let display_label = if Some(idx) == recommended_index {
            format!("{deduped}{RECOMMENDED_SUFFIX}")
        } else {
            deduped.clone()
        };
        let description = choice
            .description
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(redact_credentials_text);
        prepared.push(PreparedChoice {
            display_label,
            deduped_label: deduped,
            description,
        });
    }
    prepared
}

fn disambiguate_label(base: &str, counts: &mut HashMap<String, usize>, used: &mut HashSet<String>) -> String {
    let count = counts.entry(base.to_string()).or_insert(0);
    *count += 1;
    let mut suffix = *count;
    let mut candidate = if suffix == 1 {
        base.to_string()
    } else {
        format!("{base} ({suffix})")
    };
    while !used.insert(candidate.clone()) {
        suffix += 1;
        candidate = format!("{base} ({suffix})");
    }
    candidate
}

fn build_modal_request(questions: &[PreparedQuestion]) -> A2uiModalRequest {
    let title = modal_title(questions);
    let mut components = Vec::new();
    let mut root_children = Vec::new();
    let mut initial_answers = Map::new();

    for q in questions {
        let field_id = format!("q_{}", q.id);
        components.push(build_question_component(&field_id, q));

        if let Some(header) = &q.redacted_header {
            let header_id = format!("header_{}", q.id);
            let group_id = format!("group_{}", q.id);
            components.push(A2uiComponent::new(
                header_id.clone(),
                A2uiComponentKind::Text {
                    text: BoundString::literal(header.clone()),
                    usage_hint: TextUsageHint::H3,
                    tone: Some(SemanticTone::Accent),
                },
            ));
            components.push(A2uiComponent::new(
                group_id.clone(),
                A2uiComponentKind::Column {
                    children: vec![header_id, field_id],
                    gap: Some(0),
                },
            ));
            root_children.push(group_id);
        } else {
            root_children.push(field_id);
        }

        initial_answers.insert(q.id.clone(), initial_answer_value(q));
    }

    append_action_buttons(&mut components, &mut root_children);
    components.push(A2uiComponent::new(
        "root",
        A2uiComponentKind::Column {
            children: root_children,
            gap: Some(1),
        },
    ));

    let messages = vec![
        A2uiMessage::CreateSurface(CreateSurface {
            surface_id: ASK_SURFACE_ID.to_string(),
            catalog_id: None,
            placement: SurfacePlacement::Modal,
            title: Some(title.clone()),
            priority: 100,
            root_id: "root".to_string(),
        }),
        A2uiMessage::UpdateComponents(UpdateComponents {
            surface_id: ASK_SURFACE_ID.to_string(),
            components,
        }),
        A2uiMessage::UpdateDataModel(UpdateDataModel {
            surface_id: ASK_SURFACE_ID.to_string(),
            path: None,
            value: json!({ "answers": initial_answers }),
        }),
    ];

    A2uiModalRequest {
        surface_id: ASK_SURFACE_ID.to_string(),
        title: Some(title),
        messages,
        timeout_ms: None,
    }
}

fn modal_title(questions: &[PreparedQuestion]) -> String {
    if questions.len() == 1
        && let Some(header) = &questions[0].redacted_header
    {
        return header.clone();
    }
    "Clarification".to_string()
}

fn build_question_component(field_id: &str, q: &PreparedQuestion) -> A2uiComponent {
    if q.choices.is_empty() {
        return A2uiComponent::new(
            field_id,
            A2uiComponentKind::TextField {
                label: Some(BoundString::literal(q.redacted_question.clone())),
                placeholder: Some("Type your answer...".to_string()),
                value_path: format!("/answers/{}", q.id),
                multiline: false,
                masked: false,
            },
        );
    }

    let options = q
        .choices
        .iter()
        .enumerate()
        .map(|(idx, choice)| ChoiceOption {
            value: idx.to_string(),
            label: BoundString::literal(choice.display_label.clone()),
            description: choice.description.clone().map(BoundString::literal),
        })
        .collect();

    A2uiComponent::new(
        field_id,
        A2uiComponentKind::ChoicePicker {
            label: Some(BoundString::literal(q.redacted_question.clone())),
            options,
            selection_path: format!("/answers/{}", q.id),
            multi_select: q.multi_select,
            allow_other: q.allow_other,
            other_path: q.allow_other.then(|| format!("/answers/{}_other", q.id)),
        },
    )
}

fn initial_answer_value(q: &PreparedQuestion) -> Value {
    if q.choices.is_empty() {
        return Value::String(String::new());
    }
    if q.multi_select {
        return Value::Array(Vec::new());
    }
    match q.recommended_index {
        Some(idx) => Value::String(idx.to_string()),
        None => Value::Null,
    }
}

fn append_action_buttons(components: &mut Vec<A2uiComponent>, root_children: &mut Vec<String>) {
    components.push(A2uiComponent::new(
        "cancel_btn",
        A2uiComponentKind::Button {
            label: BoundString::literal("Cancel"),
            action: ActionBinding {
                name: "cancel".to_string(),
                context: BTreeMap::new(),
            },
            variant: ButtonVariant::Secondary,
        },
    ));
    components.push(A2uiComponent::new(
        "submit_btn",
        A2uiComponentKind::Button {
            label: BoundString::literal("Submit"),
            action: ActionBinding {
                name: "submit".to_string(),
                context: BTreeMap::new(),
            },
            variant: ButtonVariant::Primary,
        },
    ));
    components.push(A2uiComponent::new(
        "actions",
        A2uiComponentKind::Row {
            children: vec!["cancel_btn".to_string(), "submit_btn".to_string()],
            gap: Some(1),
            justify: Some(RowJustify::End),
        },
    ));
    root_children.push("actions".to_string());
}

fn build_output(questions: &[PreparedQuestion], result: Option<A2uiModalResult>) -> AskOutput {
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
    let answer_val = data_model.pointer(&format!("/answers/{}", q.id));
    let other_text = data_model
        .pointer(&format!("/answers/{}_other", q.id))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decision::{DecisionAnswer, DecisionModel, DecisionResponse};
    use rho_harness_core::a2ui::A2uiUserAction;
    use rho_harness_core::presentation::{ActivityToken, SessionStatus, ToolLine, ToolStreamPort, WelcomeDisplay};
    use std::sync::Mutex;

    struct MockPresenter {
        next_result: Mutex<Option<Option<A2uiModalResult>>>,
        captured_requests: Mutex<Vec<A2uiModalRequest>>,
    }

    impl MockPresenter {
        fn returning(result: Option<A2uiModalResult>) -> Arc<Self> {
            Arc::new(Self {
                next_result: Mutex::new(Some(result)),
                captured_requests: Mutex::new(Vec::new()),
            })
        }

        fn requests(&self) -> Vec<A2uiModalRequest> {
            self.captured_requests.lock().unwrap().clone()
        }
    }

    #[async_trait::async_trait]
    impl Presenter for MockPresenter {
        fn write_output(&self, _text: &str) {}
        fn print_welcome(&self, _display: &WelcomeDisplay) {}
        fn print_session_status(&self, _display: &SessionStatus) {}
        fn print_notice(&self, _text: &str) {}
        fn print_user_block(&self, _input: &str) {}
        fn print_token(&self, _token: &str) {}
        fn print_thinking_token(&self, _token: &str) {}
        fn finish_tool_line(&self, _line: ToolLine) {}
        fn flush(&self) {}
        fn has_interactive_ui(&self) -> bool {
            true
        }
        fn start_spinner(&self, _message: &str) -> ActivityToken {
            ActivityToken::default()
        }
        fn start_tool_spinner(&self, _name: &str, _arguments: &Value) -> ActivityToken {
            ActivityToken::default()
        }
        fn start_tool_run(&self, _name: &str, _arguments: &Value) {}
        fn stream_port(&self) -> ToolStreamPort {
            ToolStreamPort::default()
        }
        async fn request_a2ui_modal(&self, request: A2uiModalRequest) -> Option<A2uiModalResult> {
            self.captured_requests.lock().unwrap().push(request);
            self.next_result.lock().unwrap().take().flatten()
        }
    }

    #[derive(Debug)]
    struct FakeDecisionModel {
        choice: Option<String>,
        delay: Duration,
    }

    #[async_trait::async_trait]
    impl DecisionModel for FakeDecisionModel {
        fn model_name(&self) -> &str {
            "fake-decision"
        }
        fn provider_name(&self) -> &str {
            "fake"
        }
        fn is_native_decision(&self) -> bool {
            true
        }
        async fn decide(&self, _request: DecisionRequest) -> Result<DecisionResponse, AppError> {
            if !self.delay.is_zero() {
                tokio::time::sleep(self.delay).await;
            }
            let mut answers = BTreeMap::new();
            if let Some(choice) = &self.choice {
                answers.insert(
                    "choice".to_string(),
                    DecisionAnswer::Choice {
                        choice: choice.clone(),
                        probabilities: BTreeMap::new(),
                        confidence: Some(0.9),
                    },
                );
            }
            Ok(DecisionResponse {
                answers,
                ..Default::default()
            })
        }
    }

    fn submitted(data_model: Value) -> A2uiModalResult {
        A2uiModalResult::Submitted {
            action: Box::new(A2uiUserAction {
                surface_id: ASK_SURFACE_ID.to_string(),
                action_name: "submit".to_string(),
                source_component_id: "submit_btn".to_string(),
                context: json!({}),
            }),
            data_model,
        }
    }

    fn choice_picker_options(req: &A2uiModalRequest) -> Vec<ChoiceOption> {
        req.messages
            .iter()
            .find_map(|msg| match msg {
                A2uiMessage::UpdateComponents(uc) => uc.components.iter().find_map(|c| match &c.kind {
                    A2uiComponentKind::ChoicePicker { options, .. } => Some(options.clone()),
                    _ => None,
                }),
                _ => None,
            })
            .expect("ChoicePicker present")
    }

    #[tokio::test]
    async fn validation_rejects_empty_and_over_limit_inputs() {
        let presenter = MockPresenter::returning(None);
        let tool = AskTool::new(presenter);

        let err = tool.execute(json!({ "questions": [] })).await.unwrap_err();
        assert!(err.to_string().contains("at least 1 question"), "{err}");

        let too_many: Vec<Value> = (0..=MAX_QUESTIONS)
            .map(|i| json!({ "question": format!("Q{i}?") }))
            .collect();
        let err = tool.execute(json!({ "questions": too_many })).await.unwrap_err();
        assert!(err.to_string().contains("at most 5 questions"), "{err}");

        let too_many_choices: Vec<Value> = (0..=MAX_CHOICES)
            .map(|i| json!({ "label": format!("Opt {i}") }))
            .collect();
        let err = tool
            .execute(json!({
                "questions": [{ "question": "Pick one", "choices": too_many_choices }]
            }))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("maximum is 4"), "{err}");

        let err = tool
            .execute(json!({
                "questions": [{
                    "question": "Pick one",
                    "choices": [{ "label": "A" }, { "label": "B" }],
                    "recommended": 2
                }]
            }))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("out of range"), "{err}");
    }

    #[tokio::test]
    async fn recommended_suffix_is_shown_in_modal_and_stripped_from_response() {
        let presenter = MockPresenter::returning(Some(submitted(json!({
            "answers": { "q1": "1" }
        }))));
        let tool = AskTool::new(presenter.clone());

        let result = tool
            .execute(json!({
                "questions": [{
                    "question": "Database?",
                    "choices": [
                        { "label": "SQLite" },
                        { "label": "PostgreSQL" }
                    ],
                    "recommended": 1
                }]
            }))
            .await
            .unwrap();

        let requests = presenter.requests();
        let options = choice_picker_options(&requests[0]);
        assert_eq!(options[0].label, BoundString::literal("SQLite"));
        assert_eq!(options[1].label, BoundString::literal("PostgreSQL (Recommended)"));

        let output: Value = serde_json::from_str(&result.content).unwrap();
        assert_eq!(output["outcome"], "submitted");
        assert_eq!(output["answers"][0]["status"], "answered");
        assert_eq!(output["answers"][0]["selected_index"], 1);
        assert_eq!(output["answers"][0]["selected_indices"], json!([1]));
        assert_eq!(output["answers"][0]["user_response"], "PostgreSQL");
    }

    #[tokio::test]
    async fn secret_redaction_and_collision_disambiguation_map_back_to_raw_choice() {
        let raw_a = "sk-ant-api03-abcdefghijklmnop123456";
        let raw_b = "sk-ant-api03-zyxwvutsrqponmlk987654";
        let presenter = MockPresenter::returning(Some(submitted(json!({
            "answers": { "q1": "1" }
        }))));
        let tool = AskTool::new(presenter.clone());

        let result = tool
            .execute(json!({
                "questions": [{
                    "question": "Which key?",
                    "choices": [
                        { "label": raw_a },
                        { "label": raw_b }
                    ]
                }]
            }))
            .await
            .unwrap();

        let requests = presenter.requests();
        let options = choice_picker_options(&requests[0]);
        assert_eq!(options[0].label, BoundString::literal("[REDACTED] (Recommended)"));
        assert_eq!(options[1].label, BoundString::literal("[REDACTED] (2)"));

        let output: Value = serde_json::from_str(&result.content).unwrap();
        assert_eq!(output["answers"][0]["selected_index"], 1);
        assert_eq!(output["answers"][0]["user_response"], raw_b);
    }

    #[tokio::test]
    async fn other_and_free_text_questions_return_custom_text() {
        let presenter = MockPresenter::returning(Some(submitted(json!({
            "answers": {
                "q1": "__other__",
                "q1_other": "DuckDB",
                "q2": "us-east-1"
            }
        }))));
        let tool = AskTool::new(presenter);

        let result = tool
            .execute(json!({
                "questions": [
                    {
                        "question": "Engine?",
                        "choices": [{ "label": "SQLite" }, { "label": "Postgres" }]
                    },
                    {
                        "question": "Region?"
                    }
                ]
            }))
            .await
            .unwrap();

        let output: Value = serde_json::from_str(&result.content).unwrap();
        assert_eq!(output["answers"][0]["status"], "answered");
        assert_eq!(output["answers"][0]["selected_index"], Value::Null);
        assert_eq!(output["answers"][0]["user_response"], "DuckDB");
        assert_eq!(output["answers"][1]["status"], "answered");
        assert_eq!(output["answers"][1]["user_response"], "us-east-1");
    }

    #[tokio::test]
    async fn multi_select_maps_indices_back_to_raw_labels() {
        let presenter = MockPresenter::returning(Some(submitted(json!({
            "answers": {
                "q1": ["0", "2"]
            }
        }))));
        let tool = AskTool::new(presenter);

        let result = tool
            .execute(json!({
                "questions": [{
                    "question": "Features?",
                    "multi_select": true,
                    "choices": [
                        { "label": "Auth" },
                        { "label": "Billing" },
                        { "label": "Metrics" }
                    ]
                }]
            }))
            .await
            .unwrap();

        let output: Value = serde_json::from_str(&result.content).unwrap();
        assert_eq!(output["answers"][0]["status"], "answered");
        assert_eq!(output["answers"][0]["selected_index"], 0);
        assert_eq!(output["answers"][0]["selected_indices"], json!([0, 2]));
        assert_eq!(output["answers"][0]["user_response"], "Auth, Metrics");
    }

    #[tokio::test]
    async fn outcomes_cover_undelivered_cancelled_timed_out_and_skipped() {
        for (modal_result, expected_outcome, expected_status) in [
            (None, "undelivered", "unanswered"),
            (Some(A2uiModalResult::Cancelled), "cancelled", "unanswered"),
            (Some(A2uiModalResult::TimedOut), "timed_out", "unanswered"),
            (
                Some(submitted(json!({ "answers": { "q1": "" } }))),
                "submitted",
                "skipped",
            ),
        ] {
            let presenter = MockPresenter::returning(modal_result);
            let tool = AskTool::new(presenter);
            let result = tool
                .execute(json!({ "questions": [{ "question": "Optional note?" }] }))
                .await
                .unwrap();
            let output: Value = serde_json::from_str(&result.content).unwrap();
            assert_eq!(output["outcome"], expected_outcome);
            assert_eq!(output["answers"][0]["status"], expected_status);
        }
    }

    #[tokio::test]
    async fn decision_model_selects_recommended_index() {
        let presenter = MockPresenter::returning(Some(submitted(json!({
            "answers": { "q1": "1" }
        }))));
        let decision: DecisionHandle = Arc::new(FakeDecisionModel {
            choice: Some("1".to_string()),
            delay: Duration::ZERO,
        });
        let tool = AskTool::new(presenter.clone()).with_decision(decision);

        tool.execute(json!({
            "questions": [{
                "question": "Pick DB",
                "choices": [{ "label": "SQLite" }, { "label": "Postgres" }]
            }]
        }))
        .await
        .unwrap();

        let options = choice_picker_options(&presenter.requests()[0]);
        assert_eq!(options[0].label, BoundString::literal("SQLite"));
        assert_eq!(options[1].label, BoundString::literal("Postgres (Recommended)"));
    }

    #[tokio::test(start_paused = true)]
    async fn decision_model_timeout_falls_back_to_index_zero() {
        let presenter = MockPresenter::returning(Some(submitted(json!({
            "answers": { "q1": "0" }
        }))));
        let decision: DecisionHandle = Arc::new(FakeDecisionModel {
            choice: Some("1".to_string()),
            delay: Duration::from_secs(5),
        });
        let tool = AskTool::new(presenter.clone())
            .with_decision(decision)
            .with_decision_timeout(Duration::from_millis(50));

        tool.execute(json!({
            "questions": [{
                "question": "Pick DB",
                "choices": [{ "label": "SQLite" }, { "label": "Postgres" }]
            }]
        }))
        .await
        .unwrap();

        let options = choice_picker_options(&presenter.requests()[0]);
        assert_eq!(options[0].label, BoundString::literal("SQLite (Recommended)"));
        assert_eq!(options[1].label, BoundString::literal("Postgres"));
    }

    #[test]
    fn build_modal_request_groups_headers_with_zero_gap() {
        let questions = vec![
            PreparedQuestion {
                id: "q1".to_string(),
                raw_question: "Pick auth method".to_string(),
                redacted_question: "Pick auth method".to_string(),
                redacted_header: Some("Auth".to_string()),
                raw_labels: Vec::new(),
                choices: Vec::new(),
                multi_select: false,
                allow_other: false,
                recommended_index: None,
            },
            PreparedQuestion {
                id: "q2".to_string(),
                raw_question: "Enter region".to_string(),
                redacted_question: "Enter region".to_string(),
                redacted_header: None,
                raw_labels: Vec::new(),
                choices: Vec::new(),
                multi_select: false,
                allow_other: false,
                recommended_index: None,
            },
        ];
        let req = build_modal_request(&questions);
        let components = req
            .messages
            .iter()
            .find_map(|msg| match msg {
                A2uiMessage::UpdateComponents(uc) => Some(&uc.components),
                _ => None,
            })
            .expect("UpdateComponents present");

        let root = components.iter().find(|c| c.id == "root").expect("root present");
        let A2uiComponentKind::Column {
            children: root_children,
            gap: root_gap,
        } = &root.kind
        else {
            panic!("expected root Column");
        };
        assert_eq!(*root_gap, Some(1));
        assert_eq!(root_children, &vec!["group_q1", "q_q2", "actions"]);

        let group = components
            .iter()
            .find(|c| c.id == "group_q1")
            .expect("group_q1 present");
        let A2uiComponentKind::Column {
            children: group_children,
            gap: group_gap,
        } = &group.kind
        else {
            panic!("expected group_q1 Column");
        };
        assert_eq!(*group_gap, Some(0));
        assert_eq!(group_children, &vec!["header_q1", "q_q1"]);
    }
}
