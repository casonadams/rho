use std::collections::HashMap;

use rho_harness_core::model::{ChatMessage, UserContent};
use rho_harness_core::tokens::MessageTokenCache;

use crate::engine::metrics::{PruneAction, PruneDecision};

use super::{PrunePolicy, prune_with_token_cache};

/// Re-pruning a warm prefix rewrites everything after the first changed message, so a warm
/// prompt cache is only abandoned when the pruned tokens are at least this share of that rewrite.
pub const WARM_REBUILD_RATIO: f64 = 0.25;

/// Whether the provider still holds the prompt cache written by the previous request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptCache {
    Warm,
    Cold,
}

/// The tool-result stubs sent in the previous request, keyed by tool call id, so a warm
/// cache keeps seeing the exact prefix it stored.
#[derive(Debug, Default)]
pub struct PruneMemory {
    stubs: HashMap<String, UserContent>,
    tokens: MessageTokenCache,
}

impl PruneMemory {
    /// Prunes `messages` for the next request; `sizes[i]` is the token estimate of `messages[i]`.
    pub fn prune(
        &mut self,
        messages: &[ChatMessage],
        sizes: &[usize],
        policy: &PrunePolicy,
        cache: PromptCache,
    ) -> (Option<Vec<ChatMessage>>, PruneDecision) {
        let fresh = prune_with_token_cache(messages, sizes, policy, &mut self.tokens);
        let (chosen, decision) = match cache {
            PromptCache::Cold => (fresh, PruneDecision::new(PruneAction::Cold, 0, 0)),
            PromptCache::Warm => choose_for_warm_cache(
                messages,
                self.replay(messages),
                fresh,
                policy.tokenizer_model(),
                &mut self.tokens,
            ),
        };
        self.remember(messages, chosen.as_deref());
        (chosen, decision)
    }

    fn replay(&self, messages: &[ChatMessage]) -> Option<Vec<ChatMessage>> {
        if self.stubs.is_empty() {
            return None;
        }
        let mut replayed: Option<Vec<ChatMessage>> = None;
        for (idx, message) in messages.iter().enumerate() {
            let ChatMessage::User { content } = message else {
                continue;
            };
            for (part, item) in content.iter().enumerate() {
                if let Some(stub) = tool_call_id(item).and_then(|id| self.stubs.get(&id)) {
                    let target = &mut replayed.get_or_insert_with(|| messages.to_vec())[idx];
                    if let ChatMessage::User { content } = target {
                        content[part] = stub.clone();
                    }
                }
            }
        }
        replayed
    }

    fn remember(&mut self, raw: &[ChatMessage], sent: Option<&[ChatMessage]>) {
        self.stubs.clear();
        let Some(sent) = sent else {
            return;
        };
        for (raw_message, sent_message) in raw.iter().zip(sent) {
            if let (ChatMessage::User { content: before }, ChatMessage::User { content: after }) =
                (raw_message, sent_message)
            {
                for (old, new) in before.iter().zip(after).filter(|(old, new)| old != new) {
                    if let Some(id) = tool_call_id(old) {
                        self.stubs.insert(id, new.clone());
                    }
                }
            }
        }
    }
}

fn tool_call_id(item: &UserContent) -> Option<String> {
    match item {
        UserContent::ToolResult(result) => Some(result.call.to_string()),
        _ => None,
    }
}

fn suffix_tokens(memo: &mut MessageTokenCache, messages: &[ChatMessage], from: usize, model: &str) -> usize {
    memo.estimate_messages_tokens_memoized(&messages[from..], model)
}

fn choose_for_warm_cache(
    raw: &[ChatMessage],
    kept: Option<Vec<ChatMessage>>,
    fresh: Option<Vec<ChatMessage>>,
    model: &str,
    memo: &mut MessageTokenCache,
) -> (Option<Vec<ChatMessage>>, PruneDecision) {
    let kept_view = kept.as_deref().unwrap_or(raw);
    let fresh_view = fresh.as_deref().unwrap_or(raw);
    let Some(first_change) = kept_view.iter().zip(fresh_view).position(|(a, b)| a != b) else {
        return (kept, PruneDecision::new(PruneAction::Unchanged, 0, 0));
    };
    let rebuilt = suffix_tokens(memo, fresh_view, first_change, model);
    let saved = suffix_tokens(memo, kept_view, first_change, model).saturating_sub(rebuilt);
    if saved as f64 >= rebuilt as f64 * WARM_REBUILD_RATIO {
        (fresh, PruneDecision::new(PruneAction::Rewritten, saved, rebuilt))
    } else {
        (kept, PruneDecision::new(PruneAction::Kept, saved, rebuilt))
    }
}
