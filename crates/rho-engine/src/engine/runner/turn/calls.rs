use crate::engine::metrics::StructuralUsage;
use crate::engine::metrics::types::{CompletionCall, PromptResponse};

/// The model requests of one turn, which compacting its context splits into runs.
#[derive(Default)]
pub(super) struct TurnCalls {
    earlier: Vec<CompletionCall>,
    run: Vec<CompletionCall>,
}

impl TurnCalls {
    /// Records `call` of the run in progress; returns it numbered among all the turn's requests.
    pub(super) fn record(&mut self, call: CompletionCall) -> CompletionCall {
        let call = self.in_turn(call);
        self.run.push(call.clone());
        call
    }

    fn in_turn(&self, mut call: CompletionCall) -> CompletionCall {
        call.call_index += self.earlier.len();
        call
    }

    /// Ends the run in progress to start another; returns its request count.
    pub(super) fn end_run(&mut self) -> usize {
        let requests = self.run.len();
        self.earlier.append(&mut self.run);
        requests
    }

    /// The final response of the last run, with the requests of the runs before it.
    pub(super) fn whole_turn(&self, last: PromptResponse) -> PromptResponse {
        if self.earlier.is_empty() {
            return last;
        }
        let earlier = self
            .earlier
            .iter()
            .fold(StructuralUsage::default(), |sum, call| sum.merge(&call.usage));
        let mut completion_calls = self.earlier.clone();
        completion_calls.extend(last.completion_calls.into_iter().map(|call| self.in_turn(call)));
        PromptResponse {
            output: last.output,
            usage: earlier.merge(&last.usage),
            requests: self.earlier.len() + last.requests,
            completion_calls,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_response_after_a_run_ended_counts_the_requests_of_the_runs_before_it() {
        let mut calls = TurnCalls::default();
        calls.record(CompletionCall::new(0, StructuralUsage::default()));
        calls.record(CompletionCall::new(1, StructuralUsage::default()));
        assert_eq!(calls.end_run(), 2);
        let numbered = calls.record(CompletionCall::new(0, StructuralUsage::default()));
        assert_eq!(numbered.call_index, 2);
        assert_eq!(calls.end_run(), 1);

        let mut last = PromptResponse::new("done", StructuralUsage::default());
        last.requests = 1;
        last.completion_calls = vec![CompletionCall::new(0, StructuralUsage::default())];
        let whole = calls.whole_turn(last);

        assert_eq!(whole.output, "done");
        assert_eq!(whole.requests, 4);
        let indices: Vec<usize> = whole.completion_calls.iter().map(|call| call.call_index).collect();
        assert_eq!(indices, [0, 1, 2, 3]);
    }

    #[test]
    fn a_turn_that_never_compacted_returns_its_response_unchanged() {
        let calls = TurnCalls::default();
        let mut last = PromptResponse::new("done", StructuralUsage::default());
        last.requests = 4;
        assert_eq!(calls.whole_turn(last).requests, 4);
    }
}
