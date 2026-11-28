use crate::decision::{DecisionHandle, DecisionQuestion, DecisionRequest};
use crate::tools::engine_tool::EngineTool;
use crate::tools::types::{ToolResult, generated_schema, parse_tool_args};
use answers::build_output;
use futures::future::join_all;
use modal::build_modal_request;
use rho_harness_core::error::AppError;
use rho_harness_core::presentation::Presenter;
use rho_harness_core::session::redact_credentials_text;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

mod answers;
mod modal;

pub(crate) const MAX_QUESTIONS: usize = 5;
pub(crate) const MAX_CHOICES: usize = 4;
pub(crate) const ASK_SURFACE_ID: &str = "rho:ask";
const ASK_TOOL_NAME: &str = "ask";
const DEFAULT_DECISION_TIMEOUT: Duration = Duration::from_millis(600);
const RECOMMENDED_SUFFIX: &str = " (Recommended)";

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub(crate) struct AskArgs {
    /// 1 to 5 questions to present to the user in a structured modal.
    pub questions: Vec<AskQuestionArg>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub(crate) struct AskQuestionArg {
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
pub(crate) struct AskChoiceArg {
    /// Display label for the choice.
    pub label: String,
    /// Optional supporting description for the choice.
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone)]
struct PreparedChoice {
    display_label: String,
    deduped_label: String,
    description: Option<String>,
}

#[derive(Debug, Clone)]
struct PreparedQuestion {
    /// The id the model gave or was assigned, echoed back in the answers.
    id: String,
    /// The question's own name in the modal and its data model, safe in a JSON pointer.
    key: String,
    raw_question: String,
    redacted_question: String,
    redacted_header: Option<String>,
    raw_labels: Vec<String>,
    choices: Vec<PreparedChoice>,
    multi_select: bool,
    allow_other: bool,
    recommended_index: Option<usize>,
}

pub(crate) struct AskTool {
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

    #[cfg(test)]
    pub(crate) fn with_decision_timeout(mut self, timeout: Duration) -> Self {
        self.decision_timeout = timeout;
        self
    }

    pub(crate) fn with_decision(mut self, decision: DecisionHandle) -> Self {
        self.decision = Some(decision);
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

        let prepared_all = join_all(
            args.questions
                .into_iter()
                .enumerate()
                .map(|(idx, q)| self.prepare_question(idx, q)),
        )
        .await;

        let mut prepared: Vec<PreparedQuestion> = Vec::with_capacity(prepared_all.len());
        for result in prepared_all {
            let question = result?;
            if prepared.iter().any(|earlier| earlier.id == question.id) {
                return Err(AppError::Tool(format!("question id '{}' is used twice", question.id)));
            }
            prepared.push(question);
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
            key: format!("q{}", idx + 1),
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
        let parsed: AskArgs = match parse_tool_args(ASK_TOOL_NAME, args, || self.parameters()) {
            Ok(parsed) => parsed,
            Err(invalid) => return Ok(invalid),
        };
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
        Ok(Err(error)) => {
            tracing::warn!(%error, "ask decision failed; recommending the first choice");
            0
        }
        Err(elapsed) => {
            tracing::warn!(%elapsed, "ask decision timed out; recommending the first choice");
            0
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decision::{DecisionAnswer, DecisionModel, DecisionResponse};
    use rho_harness_core::a2ui::{
        A2uiComponentKind, A2uiMessage, A2uiModalRequest, A2uiModalResult, A2uiUserAction, BoundString, ChoiceOption,
    };
    use rho_harness_core::presentation::{ActivityToken, SessionStatus, ToolLine, ToolStreamPort, WelcomeDisplay};
    use serde_json::json;
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

        let err = tool
            .execute(json!({
                "questions": [{ "id": "db", "question": "A?" }, { "id": " db ", "question": "B?" }]
            }))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("'db' is used twice"), "{err}");
    }

    #[tokio::test]
    async fn answers_reach_questions_whatever_ids_the_model_chose() {
        let presenter = MockPresenter::returning(Some(submitted(json!({
            "answers": { "q1": "first", "q2": "second", "q3": "third" }
        }))));
        let tool = AskTool::new(presenter);

        let result = tool
            .execute(json!({
                "questions": [
                    { "id": "path/to~x", "question": "Where?" },
                    { "id": "x", "question": "What?" },
                    { "id": "x_other", "question": "Else?" }
                ]
            }))
            .await
            .unwrap();

        let output: Value = serde_json::from_str(&result.content).unwrap();
        let answers: Vec<(&str, &str)> = output["answers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| (a["id"].as_str().unwrap(), a["user_response"].as_str().unwrap()))
            .collect();
        assert_eq!(answers, [("path/to~x", "first"), ("x", "second"), ("x_other", "third")]);
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

    #[tokio::test]
    async fn string_typed_booleans_and_numbers_are_coerced_to_schema_types() {
        let presenter = MockPresenter::returning(Some(submitted(json!({
            "answers": { "q1": ["0"] }
        }))));
        let tool = AskTool::new(presenter.clone());

        let result = tool
            .execute(json!({
                "questions": [{
                    "question": "Pick DBs?",
                    "multi_select": "true",
                    "recommended": "1",
                    "choices": [{ "label": "SQLite" }, { "label": "Postgres" }]
                }]
            }))
            .await
            .unwrap();

        assert!(!result.is_error, "{}", result.content);
        let request = &presenter.requests()[0];
        let picker_multi_select = request
            .messages
            .iter()
            .find_map(|msg| match msg {
                A2uiMessage::UpdateComponents(uc) => uc.components.iter().find_map(|c| match &c.kind {
                    A2uiComponentKind::ChoicePicker { multi_select, .. } => Some(*multi_select),
                    _ => None,
                }),
                _ => None,
            })
            .expect("ChoicePicker present");
        assert!(picker_multi_select);
        assert_eq!(
            choice_picker_options(request)[1].label,
            BoundString::literal("Postgres (Recommended)")
        );
    }

    #[tokio::test]
    async fn malformed_arguments_are_reported_as_tool_errors() {
        let presenter = MockPresenter::returning(None);
        let tool = AskTool::new(presenter.clone());

        let result = tool.execute(json!({ "questions": "nope" })).await.unwrap();

        assert!(result.is_error);
        assert!(
            result.content.contains("Invalid arguments for ask"),
            "{}",
            result.content
        );
        assert!(presenter.requests().is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn decision_calls_for_all_questions_run_concurrently_in_order() {
        let presenter = MockPresenter::returning(Some(submitted(json!({ "answers": {} }))));
        let decision: DecisionHandle = Arc::new(FakeDecisionModel {
            choice: Some("1".to_string()),
            delay: Duration::from_millis(500),
        });
        let tool = AskTool::new(presenter.clone())
            .with_decision(decision)
            .with_decision_timeout(Duration::from_secs(1));
        let choices = json!([{ "label": "A" }, { "label": "B" }]);
        let questions: Vec<Value> = (0..MAX_QUESTIONS)
            .map(|i| json!({ "question": format!("Q{i}?"), "choices": choices }))
            .collect();

        let started = tokio::time::Instant::now();
        tool.execute(json!({ "questions": questions })).await.unwrap();

        assert!(started.elapsed() < Duration::from_secs(1), "{:?}", started.elapsed());
        let requests = presenter.requests();
        let labels: Vec<BoundString> = requests[0]
            .messages
            .iter()
            .filter_map(|msg| match msg {
                A2uiMessage::UpdateComponents(uc) => Some(&uc.components),
                _ => None,
            })
            .flatten()
            .filter_map(|c| match &c.kind {
                A2uiComponentKind::ChoicePicker { options, .. } => Some(options[1].label.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(labels, vec![BoundString::literal("B (Recommended)"); MAX_QUESTIONS]);
    }
}
