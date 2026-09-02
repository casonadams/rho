use rho_harness_core::error::Result;
use rho_harness_core::session::SessionManager;
use std::path::Path;

pub fn prompt_session_picker(sessions_dir: &Path) -> Result<Option<String>> {
    let summaries = SessionManager::list_session_summaries(sessions_dir)?;
    if summaries.is_empty() {
        return Ok(None);
    }

    let choices: Vec<String> = summaries
        .iter()
        .map(|s| {
            let title = s.name.as_deref().unwrap_or(&s.preview);
            let time_str = s.last_modified.format("%Y-%m-%d %H:%M").to_string();
            format!("{title} ({} | {} turns | {time_str})", s.session_id, s.turn_count)
        })
        .collect();

    match inquire::Select::new("Select session to resume:", choices).prompt() {
        Ok(choice) => {
            if let Some(summary) = summaries.iter().find(|s| choice.contains(&s.session_id)) {
                Ok(Some(summary.session_id.clone()))
            } else {
                Ok(None)
            }
        }
        Err(_) => Ok(None),
    }
}
