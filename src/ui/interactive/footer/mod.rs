pub mod lines;
pub mod path;
pub mod segments;
#[cfg(test)]
mod tests;
pub mod text;

pub use lines::{
    format_footer_lines, format_footer_lines_with_theme, format_stats_line, format_top_line, is_segmented_footer,
};
pub use path::{
    abbreviate_home, cached_git_pr, get_git_branch, get_git_status_counts, get_git_status_counts_async,
    subscribe_git_pr,
};
pub use text::{fit_right_aligned, format_tokens, sanitize_status_text, truncate_to_width, truncate_with_ellipsis};
