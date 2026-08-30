use super::runner::{SubagentRunRequest, SubagentRunner};
use super::types::{AgentExecutionResult, AgentTemplate};
use rho_core::error::{AppError, Result};
use rho_sdk::contract::ProviderToolDefinition;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use tokio::sync::Semaphore;

pub struct JobEntry {
    pub job_id: String,
    pub description: String,
    pub status: String,
    pub result: Option<AgentExecutionResult>,
    pub steering_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
}

pub struct ForegroundExecutionRequest<'a> {
    pub template: &'a AgentTemplate,
    pub prompt: &'a str,
    pub available_tools: &'a [ProviderToolDefinition],
}

pub struct BackgroundSpawnRequest {
    pub template: AgentTemplate,
    pub prompt: String,
    pub description: Option<String>,
    pub available_tools: Vec<ProviderToolDefinition>,
}

#[derive(Clone)]
pub struct SubagentSupervisor {
    runner: Arc<SubagentRunner>,
    semaphore: Arc<Semaphore>,
    jobs: Arc<Mutex<BTreeMap<String, JobEntry>>>,
}

impl SubagentSupervisor {
    pub fn new(runner: Arc<SubagentRunner>, max_concurrency: usize) -> Self {
        Self {
            runner,
            semaphore: Arc::new(Semaphore::new(max_concurrency.max(1))),
            jobs: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    pub async fn run_foreground(&self, request: ForegroundExecutionRequest<'_>) -> Result<AgentExecutionResult> {
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| AppError::Plugin("Supervisor semaphore closed".to_string()))?;

        let job_id = format!("job_{}", uuid::Uuid::new_v4());
        self.runner
            .run(SubagentRunRequest {
                job_id,
                template: request.template,
                prompt: request.prompt,
                available_tools: request.available_tools,
            })
            .await
    }

    pub fn spawn_background(&self, request: BackgroundSpawnRequest) -> Result<String> {
        let BackgroundSpawnRequest {
            template,
            prompt,
            description,
            available_tools,
        } = request;

        let job_id = format!("job_{}", uuid::Uuid::new_v4());
        let desc = description.unwrap_or_else(|| format!("{}: {}", template.name, prompt));

        let (steering_tx, _steering_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

        let entry = JobEntry {
            job_id: job_id.clone(),
            description: desc,
            status: "running".to_string(),
            result: None,
            steering_tx: Some(steering_tx),
        };
        self.jobs.lock().unwrap().insert(job_id.clone(), entry);

        let runner = Arc::clone(&self.runner);
        let semaphore = Arc::clone(&self.semaphore);
        let jobs = Arc::clone(&self.jobs);
        let id_for_task = job_id.clone();

        tokio::spawn(async move {
            let _permit = match semaphore.acquire().await {
                Ok(permit) => permit,
                Err(_) => return,
            };

            let outcome = runner
                .run(SubagentRunRequest {
                    job_id: id_for_task.clone(),
                    template: &template,
                    prompt: &prompt,
                    available_tools: &available_tools,
                })
                .await;

            let mut guard = jobs.lock().unwrap();
            if let Some(job) = guard.get_mut(&id_for_task) {
                match outcome {
                    Ok(res) => {
                        job.status = "completed".to_string();
                        job.result = Some(res);
                    }
                    Err(e) => {
                        job.status = "error".to_string();
                        job.result = Some(AgentExecutionResult {
                            job_id: id_for_task,
                            status: "error".to_string(),
                            text: e.to_string(),
                            tool_calls_count: 0,
                            is_error: true,
                        });
                    }
                }
            }
        });

        Ok(job_id)
    }

    pub fn get_job_result(&self, job_id: &str) -> Option<AgentExecutionResult> {
        self.jobs.lock().unwrap().get(job_id).and_then(|j| j.result.clone())
    }

    pub fn get_job_status(&self, job_id: &str) -> Option<String> {
        self.jobs.lock().unwrap().get(job_id).map(|j| j.status.clone())
    }

    pub fn steer_job(&self, job_id: &str, message: &str) -> Result<()> {
        let guard = self.jobs.lock().unwrap();
        if let Some(job) = guard.get(job_id)
            && let Some(tx) = &job.steering_tx
        {
            tx.send(message.to_string())
                .map_err(|_| AppError::Plugin("Failed to send steering message to subagent".to_string()))?;
            return Ok(());
        }
        Err(AppError::Plugin(format!(
            "Subagent job '{job_id}' not found or completed"
        )))
    }
}
