use crate::tools::types::{ToolResult, generated_schema, into_rig_result};
use async_trait::async_trait;
use rho_sdk::capability::{CapabilityError, CapabilityId, CapabilityKind};
use rho_sdk::contract::{
    ExecutionMode, ToolCapability, ToolDescriptor, ToolHost, ToolInvocationRequest, ToolInvocationResponse,
};
use rig::tool::{Tool, ToolContext, ToolExecutionError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

pub static PROMPT_TODO: &str = include_str!("../../../../prompts/tools/todo.md");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
    Deleted,
}

impl<'de> Deserialize<'de> for TaskStatus {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let normalized = s.trim().to_ascii_lowercase().replace(['-', ' '], "_");
        match normalized.as_str() {
            "pending" => Ok(Self::Pending),
            "in_progress" | "inprogress" => Ok(Self::InProgress),
            "completed" => Ok(Self::Completed),
            "deleted" => Ok(Self::Deleted),
            other => Err(serde::de::Error::custom(format!(
                "Unknown status '{other}'. Expected pending, in_progress, completed, deleted"
            ))),
        }
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TodoAction {
    Create,
    Update,
    List,
    Get,
    Delete,
    Clear,
}

impl<'de> Deserialize<'de> for TodoAction {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match s.trim().to_ascii_lowercase().as_str() {
            "create" => Ok(Self::Create),
            "update" => Ok(Self::Update),
            "list" => Ok(Self::List),
            "get" => Ok(Self::Get),
            "delete" => Ok(Self::Delete),
            "clear" => Ok(Self::Clear),
            other => Err(serde::de::Error::custom(format!(
                "Unknown action '{other}'. Expected create, update, list, get, delete, clear"
            ))),
        }
    }
}

fn deserialize_optional_id<'de, D>(deserializer: D) -> Result<Option<usize>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let val: Option<serde_json::Value> = Deserialize::deserialize(deserializer)?;
    match val {
        Some(serde_json::Value::Number(n)) => {
            if let Some(u) = n.as_u64() {
                Ok(Some(u as usize))
            } else if let Some(f) = n.as_f64() {
                Ok(Some(f as usize))
            } else {
                Err(serde::de::Error::custom("id must be a non-negative integer"))
            }
        }
        Some(serde_json::Value::String(s)) => {
            let s = s.trim();
            if s.is_empty() {
                Ok(None)
            } else {
                s.parse::<usize>()
                    .map(Some)
                    .map_err(|_| serde::de::Error::custom(format!("Invalid integer id '{s}'")))
            }
        }
        Some(serde_json::Value::Null) | None => Ok(None),
        _ => Err(serde::de::Error::custom("id must be an integer or numeric string")),
    }
}

fn deserialize_optional_id_vec<'de, D>(deserializer: D) -> Result<Option<Vec<usize>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let val: Option<serde_json::Value> = Deserialize::deserialize(deserializer)?;
    match val {
        Some(serde_json::Value::Array(items)) => {
            let mut ids = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    serde_json::Value::Number(n) => {
                        if let Some(u) = n.as_u64() {
                            ids.push(u as usize);
                        } else if let Some(f) = n.as_f64() {
                            ids.push(f as usize);
                        }
                    }
                    serde_json::Value::String(s) => {
                        if let Ok(u) = s.trim().parse::<usize>() {
                            ids.push(u);
                        }
                    }
                    _ => {}
                }
            }
            Ok(Some(ids))
        }
        Some(serde_json::Value::Number(n)) => {
            if let Some(u) = n.as_u64() {
                Ok(Some(vec![u as usize]))
            } else {
                Ok(None)
            }
        }
        Some(serde_json::Value::String(s)) => {
            if let Ok(u) = s.trim().parse::<usize>() {
                Ok(Some(vec![u]))
            } else {
                Ok(None)
            }
        }
        Some(serde_json::Value::Null) | None => Ok(None),
        _ => Err(serde::de::Error::custom("expected an array of integer task IDs")),
    }
}

fn deserialize_optional_bool<'de, D>(deserializer: D) -> Result<Option<bool>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let val: Option<serde_json::Value> = Deserialize::deserialize(deserializer)?;
    match val {
        Some(serde_json::Value::Bool(b)) => Ok(Some(b)),
        Some(serde_json::Value::String(s)) => match s.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" => Ok(Some(true)),
            "false" | "0" | "no" => Ok(Some(false)),
            _ => Ok(None),
        },
        Some(serde_json::Value::Number(n)) => Ok(Some(n.as_u64().unwrap_or(0) != 0)),
        Some(serde_json::Value::Null) | None => Ok(None),
        _ => Ok(None),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TodoArgs {
    pub action: TodoAction,
    #[serde(default, deserialize_with = "deserialize_optional_id")]
    pub id: Option<usize>,
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub status: Option<TaskStatus>,
    #[serde(default, alias = "active_form", alias = "activeForm", rename = "activeForm")]
    pub active_form: Option<String>,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(
        default,
        alias = "blocked_by",
        alias = "blockedBy",
        rename = "blockedBy",
        deserialize_with = "deserialize_optional_id_vec"
    )]
    pub blocked_by: Option<Vec<usize>>,
    #[serde(
        default,
        alias = "add_blocked_by",
        alias = "addBlockedBy",
        rename = "addBlockedBy",
        deserialize_with = "deserialize_optional_id_vec"
    )]
    pub add_blocked_by: Option<Vec<usize>>,
    #[serde(
        default,
        alias = "remove_blocked_by",
        alias = "removeBlockedBy",
        rename = "removeBlockedBy",
        deserialize_with = "deserialize_optional_id_vec"
    )]
    pub remove_blocked_by: Option<Vec<usize>>,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
    #[serde(
        default,
        alias = "include_deleted",
        alias = "includeDeleted",
        rename = "includeDeleted",
        deserialize_with = "deserialize_optional_bool"
    )]
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
            blocked_by: blocked.into_iter().filter(|b| *b != id).collect(),
            metadata: params.metadata,
        };

        let mut tasks_guard = self.tasks.lock().unwrap();
        let mut speculative = tasks_guard.clone();
        speculative.push(task.clone());
        if has_cycle(&speculative) {
            *id_guard -= 1;
            return Err("Circular dependency detected in blockedBy relations".to_string());
        }

        tasks_guard.push(task.clone());

        Ok(format!("Created #{id}: {} ({status})", task.subject))
    }

    pub fn update(&self, params: TodoUpdateParams) -> Result<String, String> {
        let id = params.id;
        let mut tasks_guard = self.tasks.lock().unwrap();
        let task_idx = tasks_guard
            .iter()
            .position(|t| t.id == id)
            .ok_or_else(|| format!("Task #{id} not found"))?;

        let mut updated = tasks_guard[task_idx].clone();
        let old_status = updated.status;

        if let Some(sub) = params.subject {
            let sub = sub.trim().to_string();
            if !sub.is_empty() {
                updated.subject = sub;
            }
        }
        if let Some(desc) = params.description {
            updated.description = Some(desc);
        }
        if let Some(st) = params.status {
            updated.status = st;
        }
        if let Some(af) = params.active_form {
            updated.active_form = Some(af);
        }
        if let Some(ow) = params.owner {
            updated.owner = Some(ow);
        }
        if let Some(to_remove) = params.remove_blocked_by {
            let remove_set: HashSet<usize> = to_remove.into_iter().collect();
            updated.blocked_by.retain(|b| !remove_set.contains(b));
        }
        if let Some(to_add) = params.add_blocked_by {
            for b in to_add {
                if b != id && !updated.blocked_by.contains(&b) {
                    updated.blocked_by.push(b);
                }
            }
        }
        if let Some(new_meta) = params.metadata {
            if let Some(new_obj) = new_meta.as_object() {
                let mut base_map = match &updated.metadata {
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
                updated.metadata = Some(serde_json::Value::Object(base_map));
            } else {
                updated.metadata = Some(new_meta);
            }
        }

        // Check for dependency cycles using speculative update
        let mut speculative = tasks_guard.clone();
        speculative[task_idx] = updated.clone();
        if has_cycle(&speculative) {
            return Err("Circular dependency detected in blockedBy relations".to_string());
        }

        tasks_guard[task_idx] = updated;

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
        let schema = generated_schema::<TodoArgs>();
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
                let Some(subject) = args.subject.filter(|s| !s.trim().is_empty()) else {
                    return Ok(ToolResult::error("Task 'subject' is required for create"));
                };
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
                if args.subject.is_none()
                    && args.description.is_none()
                    && args.status.is_none()
                    && args.active_form.is_none()
                    && args.owner.is_none()
                    && args.add_blocked_by.is_none()
                    && args.remove_blocked_by.is_none()
                    && args.metadata.is_none()
                {
                    return Ok(ToolResult::error("At least one field to update must be provided"));
                }
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

impl Tool for TodoTool {
    const NAME: &'static str = "todo";
    type Args = TodoArgs;
    type Output = String;
    type Error = ToolExecutionError;

    fn description(&self) -> String {
        "Manage a task list for tracking multi-step progress. Actions: create, update, list, get, delete, clear."
            .to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        generated_schema::<TodoArgs>()
    }

    async fn call(
        &self,
        _context: &mut ToolContext,
        args: Self::Args,
    ) -> std::result::Result<Self::Output, Self::Error> {
        into_rig_result(self.execute(args).await)
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

        // Task 1 should not have been mutated
        let task1 = store.get(1).unwrap();
        assert!(!task1.contains("blocked by"));
    }

    #[tokio::test]
    async fn test_todo_schema_compilation_and_validation() {
        let schema_val = crate::tools::types::generated_schema::<TodoArgs>();
        let compiled = rho_sdk::schema::CompiledSchema::compile(&schema_val).expect("Schema must compile");

        // Assert schema has no $defs, $ref, or $schema left over
        assert!(schema_val.get("$defs").is_none());
        assert!(schema_val.get("definitions").is_none());
        assert!(schema_val.get("$schema").is_none());

        let valid_cases = vec![
            ("list plain", serde_json::json!({"action": "list"})),
            (
                "create with subject",
                serde_json::json!({"action": "create", "subject": "test"}),
            ),
            (
                "create with description",
                serde_json::json!({"action": "create", "subject": "test", "description": "foo"}),
            ),
            (
                "create with empty blockedBy",
                serde_json::json!({"action": "create", "subject": "test", "blockedBy": []}),
            ),
            (
                "create with snake_case active_form",
                serde_json::json!({"action": "create", "subject": "test", "active_form": "doing things"}),
            ),
            (
                "create with snake_case blocked_by",
                serde_json::json!({"action": "create", "subject": "test", "blocked_by": []}),
            ),
            (
                "update with id and status",
                serde_json::json!({"action": "update", "id": 1, "status": "in_progress"}),
            ),
            (
                "create with status pending",
                serde_json::json!({"action": "create", "subject": "test", "status": "pending"}),
            ),
            (
                "list with status pending",
                serde_json::json!({"action": "list", "status": "pending"}),
            ),
            (
                "list with snake include_deleted",
                serde_json::json!({"action": "list", "include_deleted": false}),
            ),
            (
                "list with camel includeDeleted",
                serde_json::json!({"action": "list", "includeDeleted": false}),
            ),
            ("get with int id", serde_json::json!({"action": "get", "id": 1})),
            ("delete with int id", serde_json::json!({"action": "delete", "id": 1})),
            ("clear plain", serde_json::json!({"action": "clear"})),
        ];

        for (label, case) in valid_cases {
            assert!(
                compiled.validate(&case).is_ok(),
                "Schema validation failed for '{label}': {case}"
            );
            assert!(
                serde_json::from_value::<TodoArgs>(case.clone()).is_ok(),
                "Serde deserialization failed for '{label}': {case}"
            );
        }

        // Additional flexible serde deserialization cases
        let serde_flexible_cases = vec![
            (
                "update with string id",
                serde_json::json!({"action": "update", "id": "1", "status": "in_progress"}),
            ),
            ("get with string id", serde_json::json!({"action": "get", "id": "1"})),
            (
                "delete with string id",
                serde_json::json!({"action": "delete", "id": "1"}),
            ),
            (
                "capitalized action",
                serde_json::json!({"action": "Create", "subject": "test"}),
            ),
            (
                "pascal status",
                serde_json::json!({"action": "create", "subject": "test", "status": "InProgress"}),
            ),
            (
                "kebab status",
                serde_json::json!({"action": "create", "subject": "test", "status": "in-progress"}),
            ),
        ];

        for (label, case) in serde_flexible_cases {
            assert!(
                serde_json::from_value::<TodoArgs>(case.clone()).is_ok(),
                "Serde flexible deserialization failed for '{label}': {case}"
            );
        }

        // Test executing tool directly with flexible args
        let tool = TodoTool::new(TodoStore::new());
        let res = tool
            .execute(
                serde_json::from_value(serde_json::json!({
                    "action": "CREATE",
                    "subject": "flexible task",
                    "status": "InProgress",
                    "active_form": "running tests"
                }))
                .unwrap(),
            )
            .await
            .unwrap();
        assert!(!res.is_error);
        assert!(res.content.contains("Created #1: flexible task (in_progress)"));

        let res2 = tool
            .execute(
                serde_json::from_value(serde_json::json!({
                    "action": "update",
                    "id": "1",
                    "status": "completed"
                }))
                .unwrap(),
            )
            .await
            .unwrap();
        assert!(!res2.is_error);
        assert!(res2.content.contains("Updated #1 (in_progress → completed)"));
    }
}
