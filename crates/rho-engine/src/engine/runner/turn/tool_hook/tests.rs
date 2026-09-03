use super::{gated_result, text_render, with_omission_note};
use rig::agent::hook::ToolResultAction;
use rig::completion::message::{DocumentSourceKind, Image, ImageMediaType, ToolResultContent};
use rig::tool::ToolOutput;

fn text_output(text: &str) -> ToolOutput {
    ToolOutput::text(text)
}

fn image_block(media_type: Option<ImageMediaType>) -> ToolResultContent {
    ToolResultContent::Image(Image {
        data: DocumentSourceKind::Base64("aGVsbG8=".to_string()),
        media_type,
        ..Image::default()
    })
}

fn image_output(media_type: Option<rig::completion::message::ImageMediaType>) -> ToolOutput {
    ToolOutput::content(vec![
        ToolResultContent::text("Read image file"),
        image_block(media_type),
    ])
    .expect("non-empty")
}

#[test]
fn plain_text_renders_byte_identical() {
    let (text, has_images) = text_render(&text_output("hello world"));
    assert_eq!(text, "hello world");
    assert!(!has_images);
}

#[test]
fn json_output_renders_as_json_without_images() {
    let output = ToolOutput::content(vec![ToolResultContent::json(serde_json::json!({"a": 1}))]).expect("non-empty");
    let (text, has_images) = text_render(&output);
    assert_eq!(text, r#"{"a":1}"#);
    assert!(!has_images);
}

#[test]
fn image_blocks_render_as_placeholders() {
    let (text, has_images) = text_render(&image_output(Some(rig::completion::message::ImageMediaType::PNG)));
    assert_eq!(text, "Read image file\n[image: image/png]");
    assert!(has_images);
}

#[test]
fn image_without_media_type_renders_unknown_placeholder() {
    let (text, _) = text_render(&image_output(None));
    assert_eq!(text, "Read image file\n[image: unknown]");
}

#[test]
fn capable_providers_keep_image_results() {
    for provider in ["anthropic", "gemini", "chatgpt"] {
        let (action, display) = gated_result(
            &image_output(Some(rig::completion::message::ImageMediaType::PNG)),
            provider,
        );
        assert_eq!(action, ToolResultAction::keep());
        assert_eq!(display, "Read image file\n[image: image/png]");
    }
}

#[test]
fn incapable_providers_get_image_results_rewritten_with_note() {
    let (action, display) = gated_result(
        &image_output(Some(rig::completion::message::ImageMediaType::JPEG)),
        "openai",
    );
    let expected = "Read image file\n[image: image/jpeg]\n[Image in tool result omitted: openai does not support images in tool results.]";
    assert_eq!(action, ToolResultAction::rewrite(expected));
    assert_eq!(display, expected);
}

#[test]
fn unknown_providers_get_image_results_rewritten() {
    let (action, _) = gated_result(&image_output(None), "my-custom-provider");
    assert_ne!(action, ToolResultAction::keep());
}

#[test]
fn text_results_pass_through_for_every_provider() {
    for provider in ["anthropic", "openai", "unknown"] {
        let (action, display) = gated_result(&text_output("plain text"), provider);
        assert_eq!(action, ToolResultAction::keep());
        assert_eq!(display, "plain text");
    }
}

#[test]
fn omission_note_stands_alone_when_no_text_parts() {
    assert_eq!(
        with_omission_note("", "openai"),
        "[Image in tool result omitted: openai does not support images in tool results.]"
    );
}
