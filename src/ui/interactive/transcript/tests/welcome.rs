use crate::ui::interactive::transcript::{TranscriptItem, TranscriptRenderInput, WelcomeItem, render_transcript_item};
use crate::ui::theme::Theme;

fn render_welcome(item: &TranscriptItem, theme: &Theme) -> String {
    render_transcript_item(TranscriptRenderInput {
        item,
        theme,
        width: 80,
        tools_expanded: false,
        hide_thinking: false,
    })
}

fn assert_welcome_content(rendered: &str, expected: &[&str]) {
    for text in expected {
        assert!(rendered.contains(text));
    }
}

fn sample_welcome_item() -> WelcomeItem {
    WelcomeItem {
        version: "0.1.0".into(),
        model: "gpt-4".into(),
        provider: "openai".into(),
        resumed: false,
        location: ".".into(),
        agents: vec!["AGENTS.md".into()],
        tools: vec!["read".into(), "write".into(), "playwright_click".into()],
        skills: vec!["plan".into(), "spec".into()],
        plugins: vec!["permission".into()],
    }
}

#[test]
fn render_transcript_welcome() {
    let theme = Theme::default();
    let item = TranscriptItem::Welcome(sample_welcome_item());

    let rendered = render_welcome(&item, &theme);
    assert_welcome_content(
        &rendered,
        &[
            "rho",
            "Type /help for commands",
            "[agents]",
            "AGENTS.md",
            "[skills]",
            "plan, spec",
            "[tools]",
            "read, write",
            "[mcp]",
            "playwright (1 tool)",
            "[plugins]",
            "permission",
        ],
    );
}

#[test]
fn render_transcript_welcome_without_agents() {
    let theme = Theme::default();
    let item = TranscriptItem::Welcome(WelcomeItem {
        version: "0.1.0".into(),
        model: "gpt-4".into(),
        provider: "openai".into(),
        resumed: false,
        location: ".".into(),
        agents: Vec::new(),
        tools: vec!["read".into()],
        skills: Vec::new(),
        plugins: Vec::new(),
    });

    let rendered = render_transcript_item(TranscriptRenderInput {
        item: &item,
        theme: &theme,
        width: 80,
        tools_expanded: false,
        hide_thinking: false,
    });
    assert!(!rendered.contains("[agents]"));
    assert!(!rendered.contains("[skills]"));
    assert!(rendered.contains("[tools]"));
}

#[test]
fn render_transcript_welcome_wraps_long_items_on_word_boundaries() {
    let theme = Theme::default();
    let item = TranscriptItem::Welcome(WelcomeItem {
        version: "0.6.2".into(),
        model: "gpt-4".into(),
        provider: "openai".into(),
        resumed: false,
        location: ".".into(),
        agents: vec!["~/.agents/AGENTS.md".into(), "AGENTS.md".into()],
        tools: vec!["read".into(), "write".into()],
        skills: vec![
            "dream-weiver".into(),
            "google-agents-cli-adk-code".into(),
            "google-agents-cli-deploy".into(),
            "google-agents-cli-eval".into(),
            "google-agents-cli-observability".into(),
            "google-agents-cli-publish".into(),
            "google-agents-cli-scaffold".into(),
            "google-agents-cli-workflow".into(),
            "plan".into(),
            "spec".into(),
        ],
        plugins: vec!["playwright".into()],
    });

    let rendered = render_welcome(&item, &theme);
    assert!(!rendered.contains("google-a\n"));
    assert!(!rendered.contains("\n  gents-cli-eval"));
    assert!(rendered.contains("google-agents-cli-eval"));
    assert!(rendered.contains("google-agents-cli-observability"));
    assert!(rendered.contains("google-agents-cli-publish"));
    assert!(rendered.contains("google-agents-cli-scaffold"));
    assert!(rendered.contains("google-agents-cli-workflow"));
}
