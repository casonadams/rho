use crate::tools::types::ToolResult;
use async_trait::async_trait;
use rho_sdk::capability::{CapabilityError, CapabilityId, CapabilityKind};
use rho_sdk::contract::{
    ExecutionMode, ToolCapability, ToolDescriptor, ToolHost, ToolInvocationRequest, ToolInvocationResponse,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

pub static PROMPT_TODO: &str = include_str!("../../../../prompts/tools/todo.md");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
    Deleted,
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::InProgress => write!(f, "in_progress"),
            Self::Completed => write!(f, "completed"),
            Self::Deleted => write!(f, "deleted"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct TodoTask {
    pub id: usize,
    pub subject: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub status: TaskStatus,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "activeForm")]
    pub active_form: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty", rename = "blockedBy")]
    pub blocked_by: Vec<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TodoAction {
    Create,
    Update,
    List,
    Get,
    Delete,
    Clear,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TodoArgs {
    pub action: TodoAction,
    #[serde(default)]
    pub id: Option<usize>,
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub status: Option<TaskStatus>,
    #[serde(default, rename = "activeForm")]
    pub active_form: Option<String>,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default, rename = "blockedBy")]
    pub blocked_by: Option<Vec<usize>>,
    #[serde(default, rename = "addBlockedBy")]
    pub add_blocked_by: Option<Vec<usize>>,
    #[serde(default, rename = "removeBlockedBy")]
    pub remove_blocked_by: Option<Vec<usize>>,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
    #[serde(default, rename = "includeDeleted")]
    pub include_deleted: Option<bool>,
}

#[derive(Debug, Clone, Default)]
pub struct TodoCreateParams {
    pub subject: String,
    pub description: Option<String>,
    pub status: Option<TaskStatus>,
    pub active_form: Option<String>,
    pub owner: Option<String>,
    pub blocked_by: Option<Vec<usize>>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default)]
pub struct TodoUpdateParams {
    pub id: usize,
    pub subject: Option<String>,
    pub description: Option<String>,
    pub status: Option<TaskStatus>,
    pub active_form: Option<String>,
    pub owner: Option<String>,
    pub add_blocked_by: Option<Vec<usize>>,
    pub remove_blocked_by: Option<Vec<usize>>,
    pub metadata: Option<serde_json::Value>,
}

pub struct TodoStore {
    tasks: Arc<Mutex<Vec<TodoTask>>>,
    next_id: Arc<Mutex<usize>>,
}

impl Default for TodoStore {
    fn default() -> Self {
        Self::new()
    }
}

impl TodoStore {
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(Mutex::new(Vec::new())),
            next_id: Arc::new(Mutex::new(1)),
        }
    }

    pub fn create(&self, params: TodoCreateParams) -> Result<String, String> {
        let subject = params.subject.trim().to_string();
        if subject.is_empty() {
            return Err("Task subject line is required".to_string());
        }

        let mut id_guard = self.next_id.lock().unwrap();
        let id = *id_guard;
        *id_guard += 1;

        let status = params.status.unwrap_or(TaskStatus::Pending);
        let blocked = params.blocked_by.unwrap_or_default();

        let task = TodoTask {
            id,
            subject,
            description: params.description,
            status,
            active_form: params.active_form,
            owner: params.owner,
            blocked_by: blocked,
            metadata: params.metadata,
        };

        let mut tasks_guard = self.tasks.lock().unwrap();
        tasks_guard.push(task.clone());

        Ok(format!("Created #{id}: {} ({status})", task.subject))
    }

    pub fn update(&self, params: TodoUpdateParams) -> Result<String, String> {
        let id = params.id;
        let mut tasks_guard = self.tasks.lock().unwrap();
        let task = tasks_guard
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or_else(|| format!("Task #{id} not found"))?;

        let old_status = task.status;

        if let Some(sub) = params.subject {
            let sub = sub.trim().to_string();
            if !sub.is_empty() {
                task.subject = sub;
            }
        }
        if let Some(desc) = params.description {
            task.description = Some(desc);
        }
        if let Some(st) = params.status {
            task.status = st;
        }
        if let Some(af) = params.active_form {
            task.active_form = Some(af);
        }
        if let Some(ow) = params.owner {
            task.owner = Some(ow);
        }
        if let Some(to_remove) = params.remove_blocked_by {
            let remove_set: HashSet<usize> = to_remove.into_iter().collect();
            task.blocked_by.retain(|b| !remove_set.contains(b));
        }
        if let Some(to_add) = params.add_blocked_by {
            for b in to_add {
                if b != id && !task.blocked_by.contains(&b) {
                    task.blocked_by.push(b);
                }
            }
        }
        if let Some(new_meta) = params.metadata {
            if let Some(new_obj) = new_meta.as_object() {
                let mut base_map = match &task.metadata {
                    Some(serde_json::Value::Object(map)) => map.clone(),
                    _ => serde_json::Map::new(),
                };
                for (k, v) in new_obj {
                    if v.is_null() {
                        base_map.remove(k);
                    } else {
                        base_map.insert(k.clone(), v.clone());
                    }
                }
                task.metadata = Some(serde_json::Value::Object(base_map));
            } else {
                task.metadata = Some(new_meta);
            }
        }

        // Check for dependency cycles
        if has_cycle(&tasks_guard) {
            return Err("Circular dependency detected in blockedBy relations".to_string());
        }

        if let Some(new_status) = params.status
            && new_status != old_status
        {
            Ok(format!("Updated #{id} ({old_status} → {new_status})"))
        } else {
            Ok(format!("Updated #{id}"))
        }
    }

    pub fn list(&self, status_filter: Option<TaskStatus>, include_deleted: bool) -> String {
        let tasks_guard = self.tasks.lock().unwrap();
        let filtered: Vec<&TodoTask> = tasks_guard
            .iter()
            .filter(|t| {
                if !include_deleted && t.status == TaskStatus::Deleted {
                    return false;
                }
                if let Some(sf) = status_filter {
                    t.status == sf
                } else {
                    true
                }
            })
            .collect();

        if filtered.is_empty() {
            return if status_filter.is_some() {
                "No tasks match the filter.".to_string()
            } else {
                "No tasks found.".to_string()
            };
        }

        let mut lines = Vec::new();
        for t in filtered {
            let mut line = format!("#{}: [{}] {}", t.id, t.status, t.subject);
            if let Some(af) = &t.active_form
                && t.status == TaskStatus::InProgress
            {
                line.push_str(&format!(" ({af})"));
            }
            if !t.blocked_by.is_empty() {
                let blocked_strs: Vec<String> = t.blocked_by.iter().map(|id| format!("#{id}")).collect();
                line.push_str(&format!(" (blocked by {})", blocked_strs.join(", ")));
            }
            if let Some(ow) = &t.owner {
                line.push_str(&format!(" [owner: {ow}]"));
            }
            lines.push(line);
        }
        lines.join("\n")
    }

    pub fn get(&self, id: usize) -> Result<String, String> {
        let tasks_guard = self.tasks.lock().unwrap();
        let task = tasks_guard
            .iter()
            .find(|t| t.id == id)
            .ok_or_else(|| format!("Task #{id} not found"))?;

        let mut out = format!("Task #{}\nSubject: {}\nStatus: {}", task.id, task.subject, task.status);
        if let Some(desc) = &task.description {
            out.push_str(&format!("\nDescription: {desc}"));
        }
        if let Some(af) = &task.active_form {
            out.push_str(&format!("\nActive form: {af}"));
        }
        if let Some(ow) = &task.owner {
            out.push_str(&format!("\nOwner: {ow}"));
        }
        if !task.blocked_by.is_empty() {
            let blocked_strs: Vec<String> = task.blocked_by.iter().map(|id| format!("#{id}")).collect();
            out.push_str(&format!("\nBlocked by: {}", blocked_strs.join(", ")));
        }
        if let Some(meta) = &task.metadata {
            out.push_str(&format!("\nMetadata: {meta}"));
        }
        Ok(out)
    }

    pub fn delete(&self, id: usize) -> Result<String, String> {
        let mut tasks_guard = self.tasks.lock().unwrap();
        let task = tasks_guard
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or_else(|| format!("Task #{id} not found"))?;

        task.status = TaskStatus::Deleted;
        Ok(format!("Deleted #{id}: {}", task.subject))
    }

    pub fn clear(&self) -> String {
        let mut tasks_guard = self.tasks.lock().unwrap();
        tasks_guard.clear();
        let mut id_guard = self.next_id.lock().unwrap();
        *id_guard = 1;
        "Cleared all tasks.".to_string()
    }
}

fn has_cycle(tasks: &[TodoTask]) -> bool {
    let mut adj: HashMap<usize, Vec<usize>> = HashMap::new();
    for t in tasks {
        adj.insert(t.id, t.blocked_by.clone());
    }

    let mut visited: HashSet<usize> = HashSet::new();
    let mut in_stack: HashSet<usize> = HashSet::new();

    for &id in adj.keys() {
        let mut detector = CycleDetector {
            adj: &adj,
            visited: &mut visited,
            in_stack: &mut in_stack,
        };
        if !detector.visited.contains(&id) && detector.dfs(id) {
            return true;
        }
    }
    false
}

struct CycleDetector<'a> {
    adj: &'a HashMap<usize, Vec<usize>>,
    visited: &'a mut HashSet<usize>,
    in_stack: &'a mut HashSet<usize>,
}

impl<'a> CycleDetector<'a> {
    fn dfs(&mut self, node: usize) -> bool {
        self.visited.insert(node);
        self.in_stack.insert(node);

        if let Some(neighbors) = self.adj.get(&node) {
            for &neighbor in neighbors {
                if !self.visited.contains(&neighbor) {
                    if self.dfs(neighbor) {
                        return true;
                    }
                } else if self.in_stack.contains(&neighbor) {
                    return true;
                }
            }
        }

        self.in_stack.remove(&node);
        false
    }
}

pub struct TodoTool {
    store: TodoStore,
    descriptor: ToolDescriptor,
}

impl TodoTool {
    pub fn new(store: TodoStore) -> Self {
        let schema = crate::tools::types::generated_schema::<TodoArgs>();
        let descriptor = ToolDescriptor {
            id: CapabilityId::new(CapabilityKind::Tool, "todo").unwrap(),
            description: "Manage a task list for tracking multi-step progress. Actions: create, update, list, get, delete, clear.".to_string(),
            argument_schema: schema,
            prompt_guidance: PROMPT_TODO.to_string(),
            effects: Vec::new(),
            execution_mode: ExecutionMode::Sequential,
        };

        Self { store, descriptor }
    }

    pub async fn execute(&self, args: TodoArgs) -> Result<ToolResult, rho_core::error::AppError> {
        match args.action {
            TodoAction::Create => {
                let subject = args.subject.unwrap_or_default();
                match self.store.create(TodoCreateParams {
                    subject,
                    description: args.description,
                    status: args.status,
                    active_form: args.active_form,
                    owner: args.owner,
                    blocked_by: args.blocked_by,
                    metadata: args.metadata,
                }) {
                    Ok(msg) => Ok(ToolResult::success(msg)),
                    Err(e) => Ok(ToolResult::error(e)),
                }
            }
            TodoAction::Update => {
                let Some(id) = args.id else {
                    return Ok(ToolResult::error("Task 'id' is required for update"));
                };
                match self.store.update(TodoUpdateParams {
                    id,
                    subject: args.subject,
                    description: args.description,
                    status: args.status,
                    active_form: args.active_form,
                    owner: args.owner,
                    add_blocked_by: args.add_blocked_by,
                    remove_blocked_by: args.remove_blocked_by,
                    metadata: args.metadata,
                }) {
                    Ok(msg) => Ok(ToolResult::success(msg)),
                    Err(e) => Ok(ToolResult::error(e)),
                }
            }
            TodoAction::List => {
                let output = self.store.list(args.status, args.include_deleted.unwrap_or(false));
                Ok(ToolResult::success(output))
            }
            TodoAction::Get => {
                let Some(id) = args.id else {
                    return Ok(ToolResult::error("Task 'id' is required for get"));
                };
                match self.store.get(id) {
                    Ok(msg) => Ok(ToolResult::success(msg)),
                    Err(e) => Ok(ToolResult::error(e)),
                }
            }
            TodoAction::Delete => {
                let Some(id) = args.id else {
                    return Ok(ToolResult::error("Task 'id' is required for delete"));
                };
                match self.store.delete(id) {
                    Ok(msg) => Ok(ToolResult::success(msg)),
                    Err(e) => Ok(ToolResult::error(e)),
                }
            }
            TodoAction::Clear => Ok(ToolResult::success(self.store.clear())),
        }
    }
}

#[async_trait]
impl ToolCapability for TodoTool {
    fn descriptor(&self) -> ToolDescriptor {
        self.descriptor.clone()
    }

    async fn invoke(
        &self,
        _host: &dyn ToolHost,
        request: ToolInvocationRequest,
    ) -> Result<ToolInvocationResponse, CapabilityError> {
        let args: TodoArgs =
            serde_json::from_value(request.arguments).map_err(|e| CapabilityError::InvalidRequest {
                message: format!("Invalid todo arguments: {e}"),
            })?;

        match self.execute(args).await {
            Ok(res) => Ok(ToolInvocationResponse {
                content: res.content,
                is_error: res.is_error,
                structured_content: res.metadata,
            }),
            Err(e) => Err(CapabilityError::Failed { message: e.to_string() }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_todo_lifecycle_create_update_list_get_delete_clear() {
        let store = TodoStore::new();

        // 1. Create
        let res = store
            .create(TodoCreateParams {
                subject: "Task 1".to_string(),
                description: Some("Description 1".to_string()),
                status: Some(TaskStatus::Pending),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(res, "Created #1: Task 1 (pending)");

        let res2 = store
            .create(TodoCreateParams {
                subject: "Task 2".to_string(),
                status: Some(TaskStatus::Pending),
                blocked_by: Some(vec![1]),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(res2, "Created #2: Task 2 (pending)");

        // 2. List
        let list_out = store.list(None, false);
        assert!(list_out.contains("#1: [pending] Task 1"));
        assert!(list_out.contains("#2: [pending] Task 2 (blocked by #1)"));

        // 3. Update status and activeForm
        let upd_res = store
            .update(TodoUpdateParams {
                id: 1,
                status: Some(TaskStatus::InProgress),
                active_form: Some("doing task 1".to_string()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(upd_res, "Updated #1 (pending → in_progress)");

        let list_out2 = store.list(None, false);
        assert!(list_out2.contains("#1: [in_progress] Task 1 (doing task 1)"));

        // 4. Get
        let get_out = store.get(1).unwrap();
        assert!(get_out.contains("Subject: Task 1"));
        assert!(get_out.contains("Status: in_progress"));
        assert!(get_out.contains("Active form: doing task 1"));

        // 5. Complete task 1
        let upd_res2 = store
            .update(TodoUpdateParams {
                id: 1,
                status: Some(TaskStatus::Completed),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(upd_res2, "Updated #1 (in_progress → completed)");

        // 6. Delete task 2
        let del_res = store.delete(2).unwrap();
        assert_eq!(del_res, "Deleted #2: Task 2");

        let list_out3 = store.list(None, false);
        assert!(!list_out3.contains("Task 2"));
        let list_out_deleted = store.list(None, true);
        assert!(list_out_deleted.contains("Task 2"));

        // 7. Clear
        let clear_res = store.clear();
        assert_eq!(clear_res, "Cleared all tasks.");
        assert_eq!(store.list(None, false), "No tasks found.");
    }

    #[test]
    fn test_cycle_detection() {
        let store = TodoStore::new();
        store
            .create(TodoCreateParams {
                subject: "A".to_string(),
                ..Default::default()
            })
            .unwrap();
        store
            .create(TodoCreateParams {
                subject: "B".to_string(),
                blocked_by: Some(vec![1]),
                ..Default::default()
            })
            .unwrap();

        // Adding 2 as blockedBy for 1 would cause cycle 1 -> 2 -> 1
        let err = store
            .update(TodoUpdateParams {
                id: 1,
                add_blocked_by: Some(vec![2]),
                ..Default::default()
            })
            .unwrap_err();
        assert!(err.contains("Circular dependency"));
    }
}
