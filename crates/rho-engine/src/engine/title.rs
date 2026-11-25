//! Background automatic session title summarization.
//!
//! Runs a non-blocking one-shot completion on `models.smol` (falling back to the
//! active session model) with `ThinkingLevel::Off` after a turn completes, then
//! persists the result via [`SessionManager::set_auto_session_name`] without
//! overwriting user-supplied names.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use rho_harness_core::config::{Config, ModelRole};
use rho_harness_core::provider::ModelRoute;
use rho_harness_core::session::SessionManager;
use rho_harness_core::thinking::ThinkingLevel;
use tokio::sync::watch;

use super::AgentEngine;
use crate::engine::compactor::llm::ModelHandle;
use crate::provider::ModelRouter;
use crate::provider::adapter::ModelCompletionRequest;

const MAX_AUTO_TITLE_ATTEMPTS: u8 = 3;
const TITLE_TIMEOUT: Duration = Duration::from_secs(15);
const TITLE_MAX_TOKENS: u64 = 256;
const MAX_USER_INPUT_CHARS: usize = 1_400;
const MAX_ASSISTANT_INPUT_CHARS: usize = 600;
const MAX_TITLE_CHARS: usize = 80;
const MAX_TITLE_WORDS: usize = 12;

const TITLE_SYSTEM_PROMPT: &str = "\
Write a concise 3-6 word title summarizing the task in the conversation below.
- Respond ONLY with the title inside <title>...</title> tags.
- Use sentence case, no quotes, no trailing punctuation, and no prefixes like \"Title:\".
- If there is no concrete task yet (e.g. a bare greeting or vague opener like \"hi\" or \"help\"), respond with <title/>.";

#[derive(Default)]
struct TitleGenState {
    attempts: u8,
    in_flight: bool,
    cancel: Option<watch::Sender<bool>>,
}

/// Tracks background auto-title generation state and notifies subscribers when a
/// generated title is persisted.
#[derive(Clone)]
pub(crate) struct TitleTracker {
    tx: Arc<watch::Sender<u64>>,
    state: Arc<Mutex<TitleGenState>>,
}

impl Default for TitleTracker {
    fn default() -> Self {
        let (tx, _rx) = watch::channel(0u64);
        Self {
            tx: Arc::new(tx),
            state: Arc::new(Mutex::new(TitleGenState::default())),
        }
    }
}

impl TitleTracker {
    pub(crate) fn subscribe(&self) -> watch::Receiver<u64> {
        self.tx.subscribe()
    }

    pub(crate) fn cancel(&self) {
        let mut guard = self.state.lock().unwrap();
        if let Some(tx) = guard.cancel.take() {
            let _ = tx.send(true);
        }
        guard.in_flight = false;
    }

    fn try_begin(&self) -> Option<watch::Receiver<bool>> {
        let mut guard = self.state.lock().unwrap();
        if guard.in_flight || guard.attempts >= MAX_AUTO_TITLE_ATTEMPTS {
            return None;
        }
        guard.in_flight = true;
        guard.attempts += 1;
        let (tx, rx) = watch::channel(false);
        guard.cancel = Some(tx);
        Some(rx)
    }

    fn finish(&self) {
        let mut guard = self.state.lock().unwrap();
        guard.in_flight = false;
        guard.cancel = None;
    }

    fn notify_updated(&self) {
        self.tx.send_modify(|v| *v = v.wrapping_add(1));
    }
}

struct FinishGuard(TitleTracker);

impl Drop for FinishGuard {
    fn drop(&mut self) {
        self.0.finish();
    }
}

impl AgentEngine {
    pub fn title_subscribe(&self) -> watch::Receiver<u64> {
        self.title_tracker.subscribe()
    }

    pub fn cancel_auto_title(&self) {
        self.title_tracker.cancel();
    }

    /// Spawns a non-blocking background task to summarize the session into a short
    /// title when `[ui] auto_title = true` and the session has no name yet.
    pub fn maybe_spawn_auto_title(&self, user_message: &str, assistant_reply: Option<&str>) {
        if !self.config.ui.auto_title_enabled() {
            return;
        }
        if self.session_manager.cached_session_name().is_some() {
            return;
        }
        let Some(prompt) = build_title_prompt(user_message, assistant_reply) else {
            return;
        };
        let Some(cancel) = self.title_tracker.try_begin() else {
            return;
        };

        let ctx = TitleJobContext {
            config: self.config.clone(),
            router: self.router.clone(),
            fallback_model: self.model.clone(),
            session_manager: self.session_manager.clone(),
            tracker: self.title_tracker.clone(),
            prompt,
            cancel,
        };
        tokio::spawn(run_title_job(ctx));
    }
}

struct TitleJobContext {
    config: Config,
    router: ModelRouter,
    fallback_model: Option<ModelHandle>,
    session_manager: SessionManager,
    tracker: TitleTracker,
    prompt: String,
    cancel: watch::Receiver<bool>,
}

async fn run_title_job(mut ctx: TitleJobContext) {
    let _guard = FinishGuard(ctx.tracker.clone());
    let fut = generate_title_text(&ctx.config, &ctx.router, ctx.fallback_model.as_ref(), &ctx.prompt);
    let title = tokio::select! {
        biased;
        _ = ctx.cancel.wait_for(|&cancelled| cancelled) => return,
        res = tokio::time::timeout(TITLE_TIMEOUT, fut) => res.ok().flatten(),
    };
    let Some(title) = title else {
        return;
    };
    if *ctx.cancel.borrow() {
        return;
    }
    if let Ok(true) = ctx.session_manager.set_auto_session_name(&title).await {
        ctx.tracker.notify_updated();
    }
}

async fn resolve_title_model(
    config: &Config,
    router: &ModelRouter,
    fallback_model: Option<&ModelHandle>,
) -> Option<ModelHandle> {
    let off_config = Config {
        thinking_level: ThinkingLevel::Off.setting(),
        ..config.clone()
    };
    if let Some(smol_spec) = config.role_model(ModelRole::SMOL.as_str()) {
        let route = ModelRoute::parse(smol_spec, None, &config.route().connection.to_string());
        if let Ok(handle) = router.build(&off_config, &route).await {
            return Some(handle);
        }
    }
    if config.model.trim().is_empty() {
        return fallback_model.cloned();
    }
    let active_route = config.route();
    if let Ok(handle) = router.build(&off_config, &active_route).await {
        return Some(handle);
    }
    fallback_model.cloned()
}

async fn generate_title_text(
    config: &Config,
    router: &ModelRouter,
    fallback_model: Option<&ModelHandle>,
    prompt: &str,
) -> Option<String> {
    let model = resolve_title_model(config, router, fallback_model).await?;
    let request = ModelCompletionRequest {
        preamble: Some(TITLE_SYSTEM_PROMPT.to_string()),
        preamble_blocks: Vec::new(),
        prompt: prompt.to_string(),
        messages: Vec::new(),
        tools: Vec::new(),
        max_tokens: Some(TITLE_MAX_TOKENS),
        temperature: None,
        thinking_level: None,
    };
    let response = model.complete(request).await.ok()?;
    sanitize_title(&response.content)
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    let count = trimmed.chars().count();
    if count <= max_chars {
        return trimmed.to_string();
    }
    let end = trimmed
        .char_indices()
        .nth(max_chars)
        .map_or(trimmed.len(), |(idx, _)| idx);
    format!("{}...", &trimmed[..end])
}

pub(crate) fn build_title_prompt(user_message: &str, assistant_reply: Option<&str>) -> Option<String> {
    let user = truncate_chars(user_message, MAX_USER_INPUT_CHARS);
    if user.is_empty() {
        return None;
    }
    let mut out = format!("<user>\n{user}\n</user>");
    if let Some(reply) = assistant_reply {
        let assistant = truncate_chars(reply, MAX_ASSISTANT_INPUT_CHARS);
        if !assistant.is_empty() {
            out.push_str("\n<assistant>\n");
            out.push_str(&assistant);
            out.push_str("\n</assistant>");
        }
    }
    Some(out)
}

fn strip_tag_block(input: &str, open_tag: &str, close_tag: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut rest = input;
    loop {
        let lower = rest.to_ascii_lowercase();
        let Some(start) = lower.find(open_tag) else {
            result.push_str(rest);
            break;
        };
        result.push_str(&rest[..start]);
        let after_open = start + open_tag.len();
        if let Some(end_rel) = lower[after_open..].find(close_tag) {
            rest = &rest[after_open + end_rel + close_tag.len()..];
        } else {
            break;
        }
    }
    result
}

fn strip_thinking_blocks(raw: &str) -> String {
    let s = strip_tag_block(raw, "<think>", "</think>");
    let s = strip_tag_block(&s, "<thinking>", "</thinking>");
    strip_tag_block(&s, "<reasoning>", "</reasoning>")
}

fn extract_title_candidate(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    if lower.contains("<title/>") || lower.contains("<title />") {
        return None;
    }
    if let Some(start) = lower.find("<title>") {
        let after = start + "<title>".len();
        let end = lower[after..].find("</title>")?;
        return Some(text[after..after + end].to_string());
    }
    let trimmed = text.trim();
    if trimmed.starts_with('{')
        && let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed)
        && let Some(title) = val.get("title").and_then(|v| v.as_str())
    {
        return Some(title.to_string());
    }
    trimmed.lines().find(|line| !line.trim().is_empty()).map(str::to_string)
}

fn strip_title_prefix(s: &str) -> &str {
    for prefix in ["title:", "session title:", "summary:"] {
        if s.len() >= prefix.len() && s[..prefix.len()].eq_ignore_ascii_case(prefix) {
            return s[prefix.len()..].trim();
        }
    }
    s
}

/// Extracts and normalizes a model-generated title, returning `None` when the
/// model declined (`<title/>` / `none`) or emitted an invalid/overlong response.
pub fn sanitize_title(raw: &str) -> Option<String> {
    let visible = strip_thinking_blocks(raw);
    let candidate = extract_title_candidate(&visible)?;
    let no_ctrl: String = candidate
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let collapsed = no_ctrl.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = strip_title_prefix(collapsed.trim())
        .trim_matches(['"', '\'', '`'])
        .trim()
        .trim_end_matches(['.', '!', '?', ':', ';', ','])
        .trim();

    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("none") {
        return None;
    }
    if trimmed.chars().count() > MAX_TITLE_CHARS || trimmed.split_whitespace().count() > MAX_TITLE_WORDS {
        return None;
    }
    Some(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::eval::mock::{MockEngineConfig, mock_engine};
    use crate::provider::adapter::MockAdapter;
    use rho_harness_core::config::UiConfig;
    use rho_harness_core::session::TitleSource;
    use rho_test_support::TestDir;

    #[test]
    fn sanitize_extracts_title_tag_and_strips_thinking() {
        let raw = "<think>User wants <title>wrong</title> stuff</think>\n<title>Add auto session titles.</title>";
        assert_eq!(sanitize_title(raw).as_deref(), Some("Add auto session titles"));
    }

    #[test]
    fn sanitize_rejects_declined_or_empty_or_overlong() {
        assert_eq!(sanitize_title("<title/>"), None);
        assert_eq!(sanitize_title("<title />"), None);
        assert_eq!(sanitize_title("<title>none</title>"), None);
        assert_eq!(sanitize_title("<title>   </title>"), None);
        assert_eq!(
            sanitize_title("<title>one two three four five six seven eight nine ten eleven twelve thirteen</title>"),
            None
        );
        let too_long = format!("<title>{}</title>", "a".repeat(81));
        assert_eq!(sanitize_title(&too_long), None);
    }

    #[test]
    fn sanitize_handles_json_and_strips_control_chars_and_prefixes() {
        assert_eq!(
            sanitize_title(r#"{"title": "Title: \"Fix\u001b[31m auth bug!\""}"#).as_deref(),
            Some("Fix [31m auth bug")
        );
    }

    #[test]
    fn build_title_prompt_includes_user_and_assistant() {
        assert!(build_title_prompt("   ", None).is_none());
        let prompt = build_title_prompt("Fix login bug", Some("I updated auth.rs")).unwrap();
        assert!(prompt.contains("<user>\nFix login bug\n</user>"));
        assert!(prompt.contains("<assistant>\nI updated auth.rs\n</assistant>"));
    }

    #[tokio::test]
    async fn maybe_spawn_auto_title_persists_and_notifies_when_enabled() {
        let dir = TestDir::new();
        let adapter = Arc::new(MockAdapter::text("<title>Fix login redirect loop</title>"));
        let engine = mock_engine(
            adapter,
            MockEngineConfig {
                base_dir: &dir,
                app_config: Config {
                    ui: UiConfig {
                        auto_title: Some(true),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                session_manager: None,
                built_in_tools: None,
            },
        );

        let mut rx = engine.title_subscribe();
        engine.maybe_spawn_auto_title("Fix the login redirect loop", Some("Updated middleware."));
        tokio::time::timeout(Duration::from_secs(2), rx.changed())
            .await
            .expect("auto-title should notify promptly")
            .unwrap();

        assert_eq!(
            engine.session_manager.cached_session_name().as_deref(),
            Some("Fix login redirect loop")
        );
        assert_eq!(
            engine.session_manager.cached_session_name_source(),
            Some(TitleSource::Auto)
        );
    }

    #[tokio::test]
    async fn maybe_spawn_auto_title_skips_when_disabled_or_user_named() {
        let dir = TestDir::new();
        let adapter = Arc::new(MockAdapter::text("<title>Should not run</title>"));
        let engine = mock_engine(
            adapter,
            MockEngineConfig {
                base_dir: &dir,
                app_config: Config::default(),
                session_manager: None,
                built_in_tools: None,
            },
        );
        engine.maybe_spawn_auto_title("Do something", None);
        tokio::task::yield_now().await;
        assert_eq!(engine.session_manager.cached_session_name(), None);

        engine.session_manager.set_session_name("Custom Name").await.unwrap();
        let mut engine_enabled = engine;
        engine_enabled.config.ui.auto_title = Some(true);
        engine_enabled.maybe_spawn_auto_title("Do something", None);
        tokio::task::yield_now().await;
        assert_eq!(
            engine_enabled.session_manager.cached_session_name().as_deref(),
            Some("Custom Name")
        );
    }
}
