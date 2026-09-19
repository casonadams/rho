pub mod lines;
pub mod path;
#[cfg(test)]
mod tests;
pub mod text;

pub use lines::{format_footer_lines, format_stats_line, format_top_line};
pub use path::{abbreviate_home, get_git_branch};
pub use text::{
    fit_right_aligned, format_tokens, sanitize_status_text, truncate_to_width, truncate_with_ellipsis, visible_width,
};
