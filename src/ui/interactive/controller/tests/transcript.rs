//! Transcript expansion toggles and full-redraw behavior tests.

use super::fake::FakeTerminal;
use crate::ui::interactive::controller::TerminalController;

fn standard_render(controller: &TerminalController<FakeTerminal>, index: usize) -> String {
    controller.cache().entry(index).unwrap().standard.clone().unwrap()
}

mod expansion {
    use std::io;

    use super::super::fake::{FakeTerminal, Operation, controller, wrote};
    use crate::ui::interactive::controller::TerminalController;
    use crate::ui::interactive::controller::ansi::CLEAR_SCREEN;
    use crate::ui::interactive::{ToolItem, TranscriptItem};
    use test_case::test_case;

    type Setter = fn(&mut TerminalController<FakeTerminal>, bool) -> io::Result<bool>;
    type Toggle = fn(&mut TerminalController<FakeTerminal>) -> io::Result<bool>;
    type Getter = fn(&TerminalController<FakeTerminal>) -> bool;

    #[test_case(
        TranscriptItem::AssistantText("streamed response answer".into()),
        "streamed response answer";
        "assistant"
    )]
    #[test_case(
        TranscriptItem::Thinking("streamed thinking process".into()),
        "streamed thinking process";
        "thinking"
    )]
    fn pushing_streamed_text_records_it_without_duplicate_write_output(item: TranscriptItem, text: &str) {
        let (mut controller, operations, _) = controller(60);
        operations.borrow_mut().clear();

        controller.push_transcript_item(item).unwrap();

        assert_eq!(controller.transcript().len(), 1);
        assert!(
            !wrote(&operations.borrow(), text),
            "pushing already-streamed text should not write to output again"
        );
    }

    fn cache_entry_0(
        controller: &TerminalController<FakeTerminal>,
    ) -> crate::ui::interactive::controller::cache::CachedItemRender {
        controller.cache().entry(0).unwrap().clone()
    }

    #[test]
    fn full_redraw_reuses_cached_rendered_items_across_expansion_toggles() {
        let (mut controller, operations, _) = controller(60);
        let tool = TranscriptItem::tool(ToolItem {
            name: "read".into(),
            arguments: serde_json::json!({"path": "src/main.rs"}),
            is_error: false,
            output: "fn main() {}".into(),
            output_summary: "1 line".into(),
            duration_ms: None,
            file_change: None,
        });
        controller.push_transcript_item(tool).unwrap();
        assert_eq!(controller.cache().len(), 1);

        controller.toggle_tools_expanded().unwrap();
        controller.full_redraw().unwrap();
        let expanded = cache_entry_0(&controller);
        assert!(expanded.standard.is_some() && expanded.alternate.is_some());

        controller.toggle_tools_expanded().unwrap();
        controller.full_redraw().unwrap();
        let collapsed = cache_entry_0(&controller);
        assert_eq!(expanded, collapsed);

        operations.borrow_mut().clear();
        controller.full_redraw().unwrap();
        assert_eq!(collapsed, cache_entry_0(&controller));
    }

    #[test_case(TerminalController::set_tools_expanded, TerminalController::tools_expanded; "tools expanded")]
    #[test_case(TerminalController::set_hide_thinking, TerminalController::hide_thinking; "hide thinking")]
    fn setters_no_op_when_already_in_target_state(set: Setter, get: Getter) {
        let (mut controller, operations, _) = controller(60);
        assert!(!get(&controller), "starts off");

        operations.borrow_mut().clear();
        assert!(
            !set(&mut controller, false).unwrap() && operations.borrow().is_empty(),
            "off -> off"
        );

        assert!(set(&mut controller, true).unwrap() && get(&controller), "off -> on");

        operations.borrow_mut().clear();
        assert!(
            set(&mut controller, true).unwrap() && operations.borrow().is_empty(),
            "on -> on"
        );
    }

    #[test_case(TerminalController::toggle_tools_expanded, TerminalController::tools_expanded; "tools expanded")]
    #[test_case(TerminalController::toggle_thinking, TerminalController::hide_thinking; "hide thinking")]
    fn toggles_delegate_to_setter_and_report_the_new_state(toggle: Toggle, get: Getter) {
        let (mut controller, operations, _) = controller(60);

        operations.borrow_mut().clear();
        assert!(toggle(&mut controller).unwrap(), "first toggle");
        assert!(get(&controller) && !operations.borrow().is_empty(), "first toggle");

        operations.borrow_mut().clear();
        assert!(!toggle(&mut controller).unwrap(), "second toggle");
        assert!(!get(&controller) && !operations.borrow().is_empty(), "second toggle");
    }

    #[test]
    fn toggle_tools_expanded_preserves_scrollback_without_esc_3j() {
        let (mut controller, operations, _) = controller(60);
        controller
            .push_transcript_item(TranscriptItem::UserMessage("hello".into()))
            .unwrap();

        operations.borrow_mut().clear();
        controller.toggle_tools_expanded().unwrap();

        let ops = operations.borrow();
        assert!(ops.contains(&Operation::Write(CLEAR_SCREEN.into())));
        assert!(!wrote(&ops, "\x1b[3J"));
    }

    #[test]
    fn toggle_tools_expanded_repeated_cycles_preserve_cursor_and_layout() {
        let (mut controller, _, _) = controller(60);
        controller
            .push_transcript_item(TranscriptItem::UserMessage("first".into()))
            .unwrap();

        let initial_height = controller.current_layout().height();
        let initial_cursor_row = controller.current_layout().cursor_row();

        for _ in 0..5 {
            controller.toggle_tools_expanded().unwrap();
            assert!(controller.tools_expanded());

            controller.toggle_tools_expanded().unwrap();
            assert!(!controller.tools_expanded());
            assert_eq!(controller.current_layout().height(), initial_height);
            assert_eq!(controller.current_layout().cursor_row(), initial_cursor_row);
        }
    }
}

mod redraw {
    use super::super::fake::{Operation, controller, controller_with_size, write_count, wrote};
    use super::standard_render;
    use crate::ui::interactive::TranscriptItem;
    use crate::ui::interactive::controller::ansi::{
        CLEAR_SCREEN, CLEAR_SCREEN_AND_SCROLLBACK, CSI_BEGIN_SYNC_UPDATE, CSI_END_SYNC_UPDATE,
    };
    use crate::ui::theme::BlockStyle;

    #[test]
    fn resize_replays_in_flight_streamed_output() {
        let (mut controller, operations, width) = controller(60);
        controller
            .push_transcript_item(TranscriptItem::UserMessage("explain this".into()))
            .unwrap();
        controller.write_stream_output("partial streamed response").unwrap();
        operations.borrow_mut().clear();

        width.set(40);
        assert!(controller.refresh_size().unwrap());

        assert!(wrote(&operations.borrow(), "partial streamed response"));
    }

    #[test]
    fn committed_assistant_output_is_not_replayed_after_resize() {
        let (mut controller, operations, width) = controller(60);
        controller
            .push_transcript_item(TranscriptItem::UserMessage("explain this".into()))
            .unwrap();
        controller.write_stream_output("completed response").unwrap();
        controller
            .push_transcript_item(TranscriptItem::AssistantText("completed response".into()))
            .unwrap();
        operations.borrow_mut().clear();

        width.set(40);
        assert!(controller.refresh_size().unwrap());

        assert_eq!(write_count(&operations.borrow(), "completed response"), 1);
    }

    #[test]
    fn committed_thinking_output_is_not_replayed_after_resize() {
        let (mut controller, operations, width) = controller(60);
        controller.write_stream_output("considering").unwrap();
        controller
            .push_transcript_item(TranscriptItem::Thinking("considering".into()))
            .unwrap();
        operations.borrow_mut().clear();

        width.set(40);
        assert!(controller.refresh_size().unwrap());

        assert_eq!(write_count(&operations.borrow(), "considering"), 1);
    }

    #[test]
    fn full_redraw_rerenders_all_transcript_items_on_resize() {
        let (mut controller, operations, width) = controller(60);
        controller
            .push_transcript_item(TranscriptItem::UserMessage("hello world message".into()))
            .unwrap();
        operations.borrow_mut().clear();

        width.set(40);
        assert!(controller.refresh_size().unwrap());

        let ops = operations.borrow();
        assert!(ops.contains(&Operation::Write(CLEAR_SCREEN_AND_SCROLLBACK.into())));
        assert!(wrote(&ops, "hello world message"));
    }

    fn assert_sync_and_clear_order(ops: &[Operation]) {
        let sync_begin_pos = ops
            .iter()
            .position(|op| matches!(op, Operation::Write(text) if text == CSI_BEGIN_SYNC_UPDATE))
            .expect("CSI 2026h must be emitted");
        let clear_pos = ops
            .iter()
            .position(|op| matches!(op, Operation::Write(text) if text == CLEAR_SCREEN))
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
        let (mut controller, operations, _) = controller(60);
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
        let (mut controller, _, width, height) = controller_with_size(60, 24);
        controller
            .push_transcript_item(TranscriptItem::UserMessage("test message".into()))
            .unwrap();

        let initial_rendered = standard_render(&controller, 0);

        height.set(30);
        assert!(controller.refresh_size().unwrap());
        assert_eq!(initial_rendered, standard_render(&controller, 0));

        width.set(40);
        assert!(controller.refresh_size().unwrap());
        assert_ne!(initial_rendered, standard_render(&controller, 0));
    }

    fn assert_repaint(ops: &[Operation]) {
        assert!(
            ops.iter()
                .any(|op| matches!(op, Operation::Write(text) if text == CLEAR_SCREEN))
        );
        assert!(wrote(ops, "theme test message"));
        assert!(!wrote(ops, "\x1b]"));
    }

    #[test]
    fn set_theme_invalidates_cache_and_repaints_with_new_theme() {
        let (mut controller, operations, _) = controller(60);
        controller
            .push_transcript_item(TranscriptItem::UserMessage("theme test message".into()))
            .unwrap();

        let custom = crate::ui::theme::Theme {
            tool_ok: anstyle::Style::new().fg_color(Some(anstyle::Color::Rgb(anstyle::RgbColor(0x88, 0xc0, 0xd0)))),
            ..crate::ui::theme::Theme::default()
        };

        operations.borrow_mut().clear();
        controller.set_theme(custom).unwrap();

        assert_repaint(&operations.borrow());
    }

    #[test]
    fn set_block_style_switches_style_and_invalidates_cache() {
        let (mut controller, operations, _) = controller(60);
        controller
            .push_transcript_item(TranscriptItem::UserMessage("hello world".into()))
            .unwrap();

        assert_eq!(controller.block_style(), BlockStyle::Border);
        let initial_rendered = standard_render(&controller, 0);
        assert!(initial_rendered.contains('╭'));

        operations.borrow_mut().clear();
        let switched = controller.set_block_style(BlockStyle::Solid).unwrap();
        assert_eq!(switched, BlockStyle::Solid);
        assert_eq!(controller.block_style(), BlockStyle::Solid);

        let solid_rendered = standard_render(&controller, 0);
        assert_ne!(initial_rendered, solid_rendered);
        assert!(!solid_rendered.contains('╭'));

        let reverted = controller.set_block_style(BlockStyle::Border).unwrap();
        assert_eq!(reverted, BlockStyle::Border);
        assert_eq!(controller.block_style(), BlockStyle::Border);
    }

    #[test]
    fn upsert_trailing_recap_replaces_in_place_without_duplicating() {
        let (mut controller, operations, _) = controller(80);
        controller
            .push_transcript_item(TranscriptItem::UserMessage("hello".into()))
            .unwrap();

        controller.upsert_trailing_recap("first recap".into()).unwrap();
        assert_eq!(
            controller.transcript(),
            [
                TranscriptItem::UserMessage("hello".into()),
                TranscriptItem::Recap("first recap".into()),
            ]
        );
        assert!(standard_render(&controller, 1).contains("※ recap: first recap"));

        operations.borrow_mut().clear();
        controller.upsert_trailing_recap("updated recap".into()).unwrap();
        assert_eq!(
            controller.transcript(),
            [
                TranscriptItem::UserMessage("hello".into()),
                TranscriptItem::Recap("updated recap".into()),
            ]
        );
        let updated_cached = standard_render(&controller, 1);
        assert!(updated_cached.contains("※ recap: updated recap"));
        assert!(!updated_cached.contains("first recap"));
        assert!(wrote(&operations.borrow(), "updated recap"));
    }
}
