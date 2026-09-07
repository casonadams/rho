//! Transcript expansion toggles and full-redraw behavior tests.

mod expansion {
    use super::super::fake::{FakeTerminal, Operation};
    use crate::ui::interactive::controller::TerminalController;
    use crate::ui::interactive::{InteractiveState, ToolItem, TranscriptItem};

    #[test]
    fn assistant_transcript_item_is_recorded_without_duplicate_write_output() {
        let (backend, operations, _) = FakeTerminal::new(60);
        let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
        operations.borrow_mut().clear();

        controller
            .push_transcript_item(TranscriptItem::AssistantText("streamed response answer".into()))
            .unwrap();

        assert_eq!(controller.transcript().len(), 1);
        let ops = operations.borrow();
        assert!(
            !ops.iter()
                .any(|op| matches!(op, Operation::Write(text) if text.contains("streamed response answer"))),
            "pushing already-streamed assistant text should not write to output again"
        );
    }

    fn cache_entry_0(
        controller: &TerminalController<FakeTerminal>,
    ) -> crate::ui::interactive::controller::cache::CachedItemRender {
        controller.cache().entry(0).unwrap().clone()
    }

    #[test]
    fn full_redraw_reuses_cached_rendered_items_across_expansion_toggles() {
        let (backend, operations, _) = FakeTerminal::new(60);
        let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
        let tool = TranscriptItem::Tool(ToolItem {
            name: "read".into(),
            arguments: serde_json::json!({"path": "src/main.rs"}),
            is_error: false,
            output: "fn main() {}".into(),
            output_summary: "1 line".into(),
            duration_ms: None,
        });
        controller.push_transcript_item(tool).unwrap();
        assert_eq!(controller.cache().len(), 1);

        controller.toggle_tools_expanded().unwrap();
        let expanded = cache_entry_0(&controller);
        assert!(expanded.standard.is_some() && expanded.alternate.is_some());

        controller.toggle_tools_expanded().unwrap();
        let collapsed = cache_entry_0(&controller);
        assert_eq!(expanded, collapsed);

        operations.borrow_mut().clear();
        controller.full_redraw().unwrap();
        assert_eq!(collapsed, cache_entry_0(&controller));
    }

    #[test]
    fn set_tools_expanded_no_ops_when_already_in_target_state() {
        let (backend, operations, _) = FakeTerminal::new(60);
        let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
        assert!(!controller.tools_expanded());

        operations.borrow_mut().clear();
        assert!(!controller.set_tools_expanded(false).unwrap() && operations.borrow().is_empty());

        assert!(controller.set_tools_expanded(true).unwrap() && controller.tools_expanded());

        operations.borrow_mut().clear();
        assert!(controller.set_tools_expanded(true).unwrap() && operations.borrow().is_empty());
    }

    #[test]
    fn set_hide_thinking_no_ops_when_already_in_target_state() {
        let (backend, operations, _) = FakeTerminal::new(60);
        let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
        assert!(!controller.hide_thinking());

        operations.borrow_mut().clear();
        assert!(!controller.set_hide_thinking(false).unwrap() && operations.borrow().is_empty());

        assert!(controller.set_hide_thinking(true).unwrap() && controller.hide_thinking());

        operations.borrow_mut().clear();
        assert!(controller.set_hide_thinking(true).unwrap() && operations.borrow().is_empty());
    }

    #[test]
    fn toggle_tools_expanded_delegates_to_setter() {
        let (backend, operations, _) = FakeTerminal::new(60);
        let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();

        operations.borrow_mut().clear();
        assert!(controller.toggle_tools_expanded().unwrap());
        assert!(controller.tools_expanded() && !operations.borrow().is_empty());

        operations.borrow_mut().clear();
        assert!(!controller.toggle_tools_expanded().unwrap());
        assert!(!controller.tools_expanded() && !operations.borrow().is_empty());
    }

    #[test]
    fn toggle_thinking_delegates_to_setter() {
        let (backend, operations, _) = FakeTerminal::new(60);
        let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();

        operations.borrow_mut().clear();
        assert!(controller.toggle_thinking().unwrap());
        assert!(controller.hide_thinking() && !operations.borrow().is_empty());

        operations.borrow_mut().clear();
        assert!(!controller.toggle_thinking().unwrap());
        assert!(!controller.hide_thinking() && !operations.borrow().is_empty());
    }
}

mod redraw {
    use super::super::fake::{FakeTerminal, Operation};
    use crate::ui::interactive::controller::TerminalController;
    use crate::ui::interactive::controller::ansi::{CSI_BEGIN_SYNC_UPDATE, CSI_END_SYNC_UPDATE};
    use crate::ui::interactive::{InteractiveState, TranscriptItem};

    #[test]
    fn full_redraw_rerenders_all_transcript_items_on_resize() {
        let (backend, operations, width) = FakeTerminal::new(60);
        let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
        controller
            .push_transcript_item(TranscriptItem::UserMessage("hello world message".into()))
            .unwrap();
        operations.borrow_mut().clear();

        width.set(40);
        assert!(controller.refresh_size().unwrap());

        let ops = operations.borrow();
        assert!(ops.contains(&Operation::Write("\x1b[2J\x1b[H\x1b[3J\x1b[0m".into())));
        assert!(
            ops.iter()
                .any(|op| matches!(op, Operation::Write(text) if text.contains("hello world message")))
        );
    }

    fn assert_sync_and_clear_order(ops: &[Operation]) {
        let sync_begin_pos = ops
            .iter()
            .position(|op| matches!(op, Operation::Write(text) if text == CSI_BEGIN_SYNC_UPDATE))
            .expect("CSI 2026h must be emitted");
        let clear_pos = ops
            .iter()
            .position(|op| matches!(op, Operation::Write(text) if text == "\x1b[2J\x1b[H\x1b[3J\x1b[0m"))
            .expect("screen clear must be emitted");
        let sync_end_pos = ops
            .iter()
            .position(|op| matches!(op, Operation::Write(text) if text == CSI_END_SYNC_UPDATE))
            .expect("CSI 2026l must be emitted");
        let flush_pos = ops
            .iter()
            .position(|op| matches!(op, Operation::Flush))
            .expect("flush must be emitted");

        assert!(sync_begin_pos < clear_pos && clear_pos < sync_end_pos && sync_end_pos < flush_pos);
    }

    #[test]
    fn full_redraw_emits_synchronized_update_escape_codes_and_batches_output() {
        let (backend, operations, _) = FakeTerminal::new(60);
        let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
        controller
            .push_transcript_item(TranscriptItem::UserMessage("first line message".into()))
            .unwrap();
        controller
            .push_transcript_item(TranscriptItem::UserMessage("second line message".into()))
            .unwrap();
        operations.borrow_mut().clear();

        controller.full_redraw().unwrap();
        let ops = operations.borrow();
        assert_sync_and_clear_order(&ops);

        let batched_count = ops
            .iter()
            .filter(|op| {
                matches!(
                    op,
                    Operation::Write(text) if text.contains("first line message") && text.contains("second line message")
                )
            })
            .count();
        assert_eq!(batched_count, 1);
    }

    #[test]
    fn width_resize_invalidates_cache_while_height_resize_preserves_cache() {
        let (backend, _, width, height) = FakeTerminal::with_size(60, 24);
        let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
        controller
            .push_transcript_item(TranscriptItem::UserMessage("test message".into()))
            .unwrap();

        let initial_rendered = controller.cache().entry(0).unwrap().standard.clone().unwrap();

        height.set(30);
        assert!(controller.refresh_size().unwrap());
        let height_rendered = controller.cache().entry(0).unwrap().standard.clone().unwrap();
        assert_eq!(initial_rendered, height_rendered);

        width.set(40);
        assert!(controller.refresh_size().unwrap());
        let width_rendered = controller.cache().entry(0).unwrap().standard.clone().unwrap();
        assert_ne!(initial_rendered, width_rendered);
    }

    fn assert_nord_repaint(ops: &[Operation]) {
        assert!(
            ops.iter()
                .any(|op| matches!(op, Operation::Write(text) if text.contains("\x1b[2J\x1b[H\x1b[3J")))
        );
        assert!(
            ops.iter()
                .any(|op| matches!(op, Operation::Write(text) if text.starts_with("\x1b[48;2;46;52;64m\x1b[2J")))
        );
        assert!(
            ops.iter()
                .any(|op| matches!(op, Operation::Write(text) if text.contains("theme test message")))
        );
        assert!(
            !ops.iter()
                .any(|op| matches!(op, Operation::Write(text) if text.contains("\x1b]")))
        );
    }

    #[test]
    fn set_theme_invalidates_cache_and_repaints_with_new_theme() {
        let (backend, operations, _) = FakeTerminal::new(60);
        let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
        controller
            .push_transcript_item(TranscriptItem::UserMessage("theme test message".into()))
            .unwrap();

        let registry = crate::ui::theme::ThemeRegistry::default();
        let nord = registry.get("nord").unwrap().clone();

        operations.borrow_mut().clear();
        controller.set_theme(nord).unwrap();

        assert_eq!(controller.theme().name, "nord");
        assert_nord_repaint(&operations.borrow());
    }
}
