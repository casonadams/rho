use super::runner::{SubagentExecuteRequest, SubagentRunner};
use super::types::{AgentExecutionResult, AgentTemplate};
use rho_core::error::{AppError, Result};
use rho_sdk::contract::ToolHost;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use tokio::sync::Semaphore;

pub struct JobEntry {
    pub job_id: String,
    pub description: String,
    pub status: String,
    pub result: Option<AgentExecutionResult>,
    pub steering_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
    pub chunks: Arc<Mutex<Vec<String>>>,
}

pub struct SubagentTaskRequest<'a> {
    pub template: &'a AgentTemplate,
    pub prompt: &'a str,
    pub model_override: Option<&'a str>,
}

pub struct BackgroundTaskRequest {
    pub template: AgentTemplate,
    pub prompt: String,
    pub description: Option<String>,
    pub model_override: Option<String>,
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

    pub async fn run_foreground(
        &self,
        request: SubagentTaskRequest<'_>,
        host: &dyn ToolHost,
    ) -> Result<AgentExecutionResult> {
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| AppError::Plugin("Supervisor semaphore closed".to_string()))?;

        let job_id = format!("job_{}", uuid::Uuid::new_v4());
        self.runner
            .run(
                SubagentExecuteRequest {
                    job_id: Some(&job_id),
                    template: request.template,
                    prompt: request.prompt,
                    model_override: request.model_override,
                },
                host,
            )
            .await
    }

    pub fn spawn_background(&self, request: BackgroundTaskRequest) -> Result<String> {
        let job_id = format!("job_{}", uuid::Uuid::new_v4());
        let desc = request
            .description
            .unwrap_or_else(|| format!("{}: {}", request.template.name, request.prompt));

        let (steering_tx, _steering_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let chunks = Arc::new(Mutex::new(Vec::new()));

        let entry = JobEntry {
            job_id: job_id.clone(),
            description: desc,
            status: "running".to_string(),
            result: None,
            steering_tx: Some(steering_tx),
            chunks: Arc::clone(&chunks),
        };
        self.jobs.lock().unwrap().insert(job_id.clone(), entry);

        let runner = Arc::clone(&self.runner);
        let semaphore = Arc::clone(&self.semaphore);
        let jobs = Arc::clone(&self.jobs);
        let id_for_task = job_id.clone();
        let template = request.template;
        let prompt = request.prompt;
        let model_override = request.model_override;

        tokio::spawn(async move {
            let _permit = match semaphore.acquire().await {
                Ok(permit) => permit,
                Err(_) => return,
            };

            let bg_host = BackgroundToolHost {
                chunks: Arc::clone(&chunks),
            };

            let outcome = runner
                .run(
                    SubagentExecuteRequest {
                        job_id: Some(&id_for_task),
                        template: &template,
                        prompt: &prompt,
                        model_override: model_override.as_deref(),
                    },
                    &bg_host,
                )
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

    pub fn get_job(&self, job_id: &str) -> Option<JobEntrySnapshot> {
        let guard = self.jobs.lock().unwrap();
        guard.get(job_id).map(|j| {
            let chunks = j.chunks.lock().unwrap().clone();
            JobEntrySnapshot {
                job_id: j.job_id.clone(),
                description: j.description.clone(),
                status: j.status.clone(),
                result: j.result.clone(),
                output_preview: chunks.join(""),
            }
        })
    }

    pub fn steer_job(&self, job_id: &str, message: &str) -> Result<()> {
        let guard = self.jobs.lock().unwrap();
        if let Some(job) = guard.get(job_id) {
            if let Some(tx) = &job.steering_tx {
                let _ = tx.send(message.to_string());
                Ok(())
            } else {
                Err(AppError::Plugin("Steering channel closed".to_string()))
            }
        } else {
            Err(AppError::Plugin(format!("Job '{job_id}' not found")))
        }
    }
}

pub struct JobEntrySnapshot {
    pub job_id: String,
    pub description: String,
    pub status: String,
    pub result: Option<AgentExecutionResult>,
    pub output_preview: String,
}

struct BackgroundToolHost {
    chunks: Arc<Mutex<Vec<String>>>,
}

#[async_trait::async_trait]
impl ToolHost for BackgroundToolHost {
    async fn interact(
        &self,
        _request: rho_sdk::contract::InteractionRequest,
    ) -> std::result::Result<rho_sdk::contract::InteractionResponse, rho_sdk::capability::CapabilityError> {
        Err(rho_sdk::capability::CapabilityError::Unavailable {
            message: "Interactive UI unavailable for background subagents".to_string(),
        })
    }

    fn stream_chunk(&self, chunk: &str) {
        self.chunks.lock().unwrap().push(chunk.to_string());
    }
}
