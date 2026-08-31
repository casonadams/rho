use super::types::{TaskStatus, TodoCreateParams, TodoTask, TodoUpdateParams};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

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
