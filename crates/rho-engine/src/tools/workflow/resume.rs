use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

const LOG_DIR: &str = ".rho/workflows";
const JOURNAL_SUFFIX: &str = ".journal.jsonl";

/// Identifies one agent step's inputs: the same agent given the same prompt gets the same key.
pub(crate) fn step_key(agent: &str, prompt: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(agent.as_bytes());
    hasher.update([0]);
    hasher.update(prompt.as_bytes());
    hasher.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Agent results a finished or interrupted run produced, reused by a run that resumes it.
#[derive(Debug, Default)]
pub(crate) struct Replay {
    entries: HashMap<(String, String), (Value, String)>,
}

impl Replay {
    pub(crate) fn get(&self, step: &str, key: &str) -> Option<&(Value, String)> {
        self.entries.get(&(step.to_string(), key.to_string()))
    }
}

/// What a resumed run needs from the run it continues.
#[derive(Debug)]
pub(crate) struct Resumed {
    pub workflow: String,
    pub inputs: BTreeMap<String, String>,
    pub replay: Replay,
}

pub(crate) fn journal_path(log_path: &Path) -> std::path::PathBuf {
    let stem = log_path.file_stem().and_then(|s| s.to_str()).unwrap_or("run");
    log_path.with_file_name(format!("{stem}{JOURNAL_SUFFIX}"))
}

fn find_log(project_dir: &Path, run_id: &str) -> Result<std::path::PathBuf, String> {
    let dir = project_dir.join(LOG_DIR);
    let suffix = format!("-{run_id}.jsonl");
    std::fs::read_dir(&dir)
        .map_err(|e| format!("cannot read {}: {e}", dir.display()))?
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(&suffix))
        })
        .ok_or_else(|| format!("no workflow run '{run_id}' in {}", dir.display()))
}

pub(crate) fn load(project_dir: &Path, run_id: &str) -> Result<Resumed, String> {
    let log = find_log(project_dir, run_id)?;
    let text = std::fs::read_to_string(&log).map_err(|e| format!("{}: {e}", log.display()))?;
    let started: Value = text
        .lines()
        .find_map(|line| {
            serde_json::from_str::<Value>(line)
                .ok()
                .filter(|v| v["event"] == "run_started")
        })
        .ok_or_else(|| format!("{} has no run_started record", log.display()))?;
    let workflow = started["workflow"]
        .as_str()
        .ok_or_else(|| format!("{} names no workflow", log.display()))?
        .to_string();
    let inputs = serde_json::from_value(started["inputs"].clone())
        .map_err(|e| format!("{} has unreadable inputs: {e}", log.display()))?;
    let journal = journal_path(&log);
    let entries = std::fs::read_to_string(&journal)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter_map(|v| {
            let step = v["step"].as_str()?.to_string();
            let key = v["key"].as_str()?.to_string();
            let detail = v["detail"].as_str().unwrap_or_default().to_string();
            Some(((step, key), (v["value"].clone(), detail)))
        })
        .collect();
    Ok(Resumed {
        workflow,
        inputs,
        replay: Replay { entries },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDir;

    #[test]
    fn keys_depend_on_agent_and_prompt() {
        assert_eq!(step_key("a", "p"), step_key("a", "p"));
        assert_ne!(step_key("a", "p"), step_key("b", "p"));
        assert_ne!(step_key("a", "p"), step_key("a", "q"));
        assert_ne!(step_key("ab", "c"), step_key("a", "bc"));
    }

    #[test]
    fn journal_path_sits_next_to_the_run_log() {
        let path = Path::new("/p/.rho/workflows/audit-20260101-000000-abcd1234.jsonl");
        assert_eq!(
            journal_path(path),
            Path::new("/p/.rho/workflows/audit-20260101-000000-abcd1234.journal.jsonl")
        );
    }

    #[test]
    fn loads_the_workflow_inputs_and_journaled_results_of_a_run() {
        let dir = TestDir::new();
        let root: &Path = &dir;
        let logs = root.join(LOG_DIR);
        std::fs::create_dir_all(&logs).unwrap();
        std::fs::write(
            logs.join("audit-20260101-000000-abcd1234.jsonl"),
            "{\"event\":\"run_started\",\"workflow\":\"audit\",\"inputs\":{\"scope\":\"repo\"}}\n",
        )
        .unwrap();
        std::fs::write(
            logs.join("audit-20260101-000000-abcd1234.journal.jsonl"),
            "{\"step\":\"audit > recon\",\"key\":\"k1\",\"value\":{\"tasks\":[]},\"detail\":\"d\"}\nnot json\n",
        )
        .unwrap();

        let resumed = load(root, "20260101-000000-abcd1234").unwrap();

        assert_eq!(resumed.workflow, "audit");
        assert_eq!(resumed.inputs["scope"], "repo");
        let (value, detail) = resumed.replay.get("audit > recon", "k1").unwrap();
        assert_eq!((value, detail.as_str()), (&serde_json::json!({"tasks": []}), "d"));
        assert!(resumed.replay.get("audit > recon", "other").is_none());
        assert!(load(root, "missing").unwrap_err().contains("no workflow run 'missing'"));
    }
}
