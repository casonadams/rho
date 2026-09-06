use super::types::TranscriptRenderInput;
use crate::ui::block::BlockFormat;

fn render_parsed_skill(
    (skill_name, skill_content, user_msg): (&str, &str, &str),
    input: &TranscriptRenderInput<'_>,
) -> String {
    let skill_tag = input.theme.skill_tag;
    let skill_block_text = if input.tools_expanded {
        format!("{skill_tag}[skill]{skill_tag:#} **{skill_name}**\n\n{skill_content}")
    } else {
        format!("{skill_tag}[skill]{skill_tag:#} {skill_name}")
    };
    let skill_formatted = BlockFormat::new(input.theme.tool_success_bg, input.width)
        .with_vertical_padding()
        .render_styled(&skill_block_text);
    let user_trimmed = user_msg.trim();
    if user_trimmed.is_empty() {
        format!("\n{skill_formatted}")
    } else {
        let user_formatted = BlockFormat::new(input.theme.user_message_bg, input.width)
            .with_vertical_padding()
            .render_plain(user_trimmed);
        format!("\n{skill_formatted}\n{user_formatted}")
    }
}

pub fn render_user_message(text: &str, input: &TranscriptRenderInput<'_>) -> String {
    if let Some((skill_name, skill_content, user_msg)) = parse_skill_block(text) {
        render_parsed_skill((&skill_name, &skill_content, &user_msg), input)
    } else {
        let block = BlockFormat::new(input.theme.user_message_bg, input.width)
            .with_vertical_padding()
            .render_plain(text);
        format!("\n{block}")
    }
}

pub fn parse_skill_block(text: &str) -> Option<(String, String, String)> {
    let start_tag = "<skill";
    let start_idx = text.find(start_tag)?;
    let name_prefix = "name=\"";
    let name_start = text[start_idx..].find(name_prefix)? + start_idx + name_prefix.len();
    let name_end = name_start + text[name_start..].find('"')?;
    let skill_name = &text[name_start..name_end];

    let content_start = start_idx + text[start_idx..].find('>')? + 1;
    let end_tag = "</skill>";
    let end_idx = text[content_start..].find(end_tag)? + content_start;
    let skill_content = &text[content_start..end_idx];

    let user_msg = &text[end_idx + end_tag.len()..];
    let user_msg = user_msg.trim_start_matches("\n\n").trim_start_matches("Skill input: ");

    Some((
        skill_name.to_string(),
        skill_content.trim().to_string(),
        user_msg.to_string(),
    ))
}
