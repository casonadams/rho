use crate::intent::model::IntentSpec;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClarificationOption {
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClarificationQuestion {
    pub header: String,
    pub question: String,
    pub options: Vec<ClarificationOption>,
    pub allow_custom: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmbiguityAnalysis {
    pub ambiguity_score: f32,
    pub needs_clarification: bool,
    pub detected_topics: Vec<String>,
    pub questions: Vec<ClarificationQuestion>,
}

pub struct IntentAnalyzer;

impl IntentAnalyzer {
    pub fn analyze(spec: &IntentSpec) -> AmbiguityAnalysis {
        let prompt_lower = spec.objective.to_lowercase();
        let trimmed = prompt_lower.trim();

        // 1. If this is a question or informational inquiry, answer directly without blocking
        if is_informational_query(trimmed) {
            return AmbiguityAnalysis {
                ambiguity_score: 0.0,
                needs_clarification: false,
                detected_topics: vec!["informational".to_string()],
                questions: Vec::new(),
            };
        }

        let mut topics = Vec::new();
        let mut questions = Vec::new();
        let mut ambiguity_points = 0;

        let is_action_prompt = is_action_verb(trimmed);
        let word_count = trimmed.split_whitespace().count();

        if is_action_prompt && word_count < 8 {
            ambiguity_points += 2;
        }

        // Web app / Website / Frontend creation
        if (trimmed.contains("website")
            || trimmed.contains("web app")
            || trimmed.contains("frontend")
            || trimmed.contains("landing page"))
            && !trimmed.contains("react")
            && !trimmed.contains("vue")
            && !trimmed.contains("svelte")
            && !trimmed.contains("html")
        {
            topics.push("web_framework".to_string());
            ambiguity_points += 3;
            questions.push(ClarificationQuestion {
                header: "Frontend Stack".to_string(),
                question: "Which frontend stack or framework should we build with?".to_string(),
                options: vec![
                    ClarificationOption {
                        label: "Vanilla HTML/CSS/JS (Recommended)".to_string(),
                        description: "Modern, clean, zero-build-step HTML5 and JavaScript".to_string(),
                    },
                    ClarificationOption {
                        label: "React + Vite / Tailwind".to_string(),
                        description: "React single-page application with TailwindCSS".to_string(),
                    },
                    ClarificationOption {
                        label: "Next.js / SvelteKit".to_string(),
                        description: "Full-stack SSR framework".to_string(),
                    },
                ],
                allow_custom: true,
            });
        }

        // Backend / API creation
        if (trimmed.contains("api")
            || trimmed.contains("backend")
            || trimmed.contains("server")
            || trimmed.contains("microservice"))
            && is_action_prompt
            && !trimmed.contains("rest")
            && !trimmed.contains("graphql")
            && !trimmed.contains("grpc")
            && !trimmed.contains("axum")
            && !trimmed.contains("actix")
        {
            topics.push("api_architecture".to_string());
            ambiguity_points += 3;
            questions.push(ClarificationQuestion {
                header: "API Protocol".to_string(),
                question: "What API architecture or framework do you prefer?".to_string(),
                options: vec![
                    ClarificationOption {
                        label: "REST with JSON (Recommended)".to_string(),
                        description: "Standard HTTP RESTful endpoints with JSON payloads".to_string(),
                    },
                    ClarificationOption {
                        label: "GraphQL".to_string(),
                        description: "Schema-driven GraphQL query & mutation endpoint".to_string(),
                    },
                    ClarificationOption {
                        label: "gRPC / Protobuf".to_string(),
                        description: "High-performance typed binary RPC".to_string(),
                    },
                ],
                allow_custom: true,
            });
        }

        // Auth implementation
        if (trimmed.contains("auth") || trimmed.contains("authentication") || trimmed.contains("login"))
            && is_action_prompt
            && !trimmed.contains("oauth")
            && !trimmed.contains("jwt")
            && !trimmed.contains("session")
        {
            topics.push("auth_type".to_string());
            ambiguity_points += 2;
            questions.push(ClarificationQuestion {
                header: "Auth Strategy".to_string(),
                question: "What authentication mechanism do you want to implement?".to_string(),
                options: vec![
                    ClarificationOption {
                        label: "JWT with Bearer Tokens (Recommended)".to_string(),
                        description: "Stateless JSON Web Tokens with access and refresh tokens".to_string(),
                    },
                    ClarificationOption {
                        label: "Session Cookie Auth".to_string(),
                        description: "Traditional server-side cookie sessions".to_string(),
                    },
                    ClarificationOption {
                        label: "OAuth2 / Social Login".to_string(),
                        description: "Third-party login via GitHub/Google".to_string(),
                    },
                ],
                allow_custom: true,
            });
        }

        // Database / Persistence
        if (trimmed.contains("database")
            || trimmed.contains("storage")
            || trimmed.contains("store data")
            || trimmed.contains("persistence"))
            && is_action_prompt
            && !trimmed.contains("sqlite")
            && !trimmed.contains("postgres")
            && !trimmed.contains("json")
        {
            topics.push("database".to_string());
            ambiguity_points += 2;
            questions.push(ClarificationQuestion {
                header: "Storage Backend".to_string(),
                question: "Which database backend should we configure?".to_string(),
                options: vec![
                    ClarificationOption {
                        label: "SQLite (Recommended)".to_string(),
                        description: "Embedded file-based SQL database with zero infrastructure".to_string(),
                    },
                    ClarificationOption {
                        label: "PostgreSQL".to_string(),
                        description: "Full relational database server".to_string(),
                    },
                    ClarificationOption {
                        label: "Local JSON / Key-Value Store".to_string(),
                        description: "Simple file-backed persistence".to_string(),
                    },
                ],
                allow_custom: true,
            });
        }

        // Caching
        if (trimmed.contains("cache") || trimmed.contains("caching"))
            && is_action_prompt
            && !trimmed.contains("redis")
            && !trimmed.contains("lru")
            && !trimmed.contains("moka")
            && !trimmed.contains("in-memory")
        {
            topics.push("caching".to_string());
            ambiguity_points += 2;
            questions.push(ClarificationQuestion {
                header: "Cache Layer".to_string(),
                question: "Which caching strategy should we implement?".to_string(),
                options: vec![
                    ClarificationOption {
                        label: "In-Memory LRU with TTL (Recommended)".to_string(),
                        description: "Fast in-process LRU cache".to_string(),
                    },
                    ClarificationOption {
                        label: "Redis".to_string(),
                        description: "Distributed external cache store".to_string(),
                    },
                ],
                allow_custom: true,
            });
        }

        let ambiguity_score = (ambiguity_points as f32 / 5.0).clamp(0.0, 1.0);
        let needs_clarification = ambiguity_score >= 0.4 && !questions.is_empty();

        AmbiguityAnalysis {
            ambiguity_score,
            needs_clarification,
            detected_topics: topics,
            questions,
        }
    }
}

fn is_informational_query(prompt: &str) -> bool {
    let starters = [
        "what",
        "why",
        "how",
        "where",
        "who",
        "when",
        "which",
        "explain",
        "describe",
        "summarize",
        "find",
        "search",
        "show",
        "list",
        "check",
        "inspect",
        "is there",
        "are there",
        "tell me",
        "can you explain",
    ];

    for s in starters {
        if prompt.starts_with(s) {
            return true;
        }
    }

    prompt.ends_with('?')
}

fn is_action_verb(prompt: &str) -> bool {
    let verbs = [
        "build",
        "create",
        "make",
        "implement",
        "add",
        "setup",
        "scaffold",
        "write",
        "develop",
        "generate",
        "design",
        "refactor",
        "configure",
    ];

    for v in verbs {
        if prompt.starts_with(v) || prompt.contains(&format!(" {v} ")) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_informational_prompt_no_clarification() {
        let spec = IntentSpec::from_prompt("what does this repo do?");
        let analysis = IntentAnalyzer::analyze(&spec);
        assert!(!analysis.needs_clarification);
        assert_eq!(analysis.questions.len(), 0);
    }

    #[test]
    fn test_vague_action_prompt_triggers_clarification() {
        let spec = IntentSpec::from_prompt("build me a website");
        let analysis = IntentAnalyzer::analyze(&spec);
        assert!(analysis.needs_clarification);
        assert!(!analysis.questions.is_empty());
        assert_eq!(analysis.questions[0].header, "Frontend Stack");
    }

    #[test]
    fn test_api_action_prompt_triggers_clarification() {
        let spec = IntentSpec::from_prompt("create an api for users");
        let analysis = IntentAnalyzer::analyze(&spec);
        assert!(analysis.needs_clarification);
        assert!(!analysis.questions.is_empty());
        assert_eq!(analysis.questions[0].header, "API Protocol");
    }
}
