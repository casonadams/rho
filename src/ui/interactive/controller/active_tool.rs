/// Live preview cap for the active tool block; the full result still arrives
/// with the completion event, so the preview drops old bytes past this cap.
pub const MAX_ACTIVE_TOOL_OUTPUT_BYTES: usize = 32 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveToolBlock {
    pub name: String,
    pub args_summary: String,
    pub preview: Option<String>,
    pub output: String,
    pub started: std::time::Instant,
    pub(crate) truncated: bool,
}
