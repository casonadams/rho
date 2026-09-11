//! Screen-simulating backend for repaint regression tests: the fake terminal
//! maintains a physical character grid (deferred wrap, scroll, cursor moves)
//! so tests can assert that the visible screen matches the controller layout.

use crate::ui::interactive::controller::{TerminalBackend, TerminalController};
use std::io;

pub struct ScreenBackend {
    pub width: usize,
    pub height: usize,
    pub grid: Vec<Vec<char>>,
    pub filled: Vec<Vec<bool>>,
    pub row: usize,
    pub col: usize,
    pending_wrap: bool,
    bg_active: bool,
}

impl ScreenBackend {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            grid: vec![vec![' '; width]; height],
            filled: vec![vec![false; width]; height],
            row: 0,
            col: 0,
            pending_wrap: false,
            bg_active: false,
        }
    }

    fn scroll_up(&mut self) {
        self.grid.remove(0);
        self.grid.push(vec![' '; self.width]);
        self.filled.remove(0);
        self.filled.push(vec![false; self.width]);
    }

    fn newline(&mut self) {
        self.pending_wrap = false;
        self.col = 0;
        if self.row + 1 >= self.height {
            self.scroll_up();
        } else {
            self.row += 1;
        }
    }

    fn set_bg_active(&mut self, params: &str) {
        if params.is_empty() {
            self.bg_active = false;
            return;
        }
        let mut parts = params.split(';');
        while let Some(part) = parts.next() {
            match part {
                "48" => {
                    self.bg_active = true;
                    match parts.next() {
                        Some("5") => {
                            parts.next();
                        }
                        Some("2") => {
                            parts.next();
                            parts.next();
                            parts.next();
                        }
                        _ => {}
                    }
                }
                "40" | "41" | "42" | "43" | "44" | "45" | "46" | "47" => self.bg_active = true,
                "0" | "49" => self.bg_active = false,
                _ => {}
            }
        }
    }

    fn put(&mut self, c: char, w: usize) {
        if self.pending_wrap {
            self.newline();
        }
        self.grid[self.row][self.col] = c;
        if self.bg_active {
            self.filled[self.row][self.col] = true;
        }
        self.col += w;
        if self.col >= self.width {
            self.pending_wrap = true;
        }
    }

    fn text(&self) -> Vec<String> {
        self.grid
            .iter()
            .map(|row| row.iter().collect::<String>().trim_end().to_string())
            .collect()
    }

    pub fn dump(&self) -> String {
        let mut out = String::new();
        for (i, row) in self.text().iter().enumerate() {
            let any_fill = self.filled[i].iter().any(|&f| f);
            let marker = if row.trim().is_empty() && !any_fill {
                "BLANK"
            } else {
                "      "
            };
            out.push_str(&format!("{i:2} {marker}|{row}|\n", marker = marker));
        }
        out
    }

    fn run_text(&mut self, text: &str) {
        let chars: Vec<char> = text.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            match chars[i] {
                '\x1b' => i = self.run_escape(&chars, i + 1),
                '\r' => {
                    self.col = 0;
                    self.pending_wrap = false;
                    i += 1;
                }
                '\n' => {
                    self.newline();
                    i += 1;
                }
                _ => {
                    let w = unicode_width::UnicodeWidthChar::width(chars[i]).unwrap_or(0);
                    if w > 0 {
                        self.put(chars[i], w);
                    }
                    i += 1;
                }
            }
        }
    }

    fn run_escape(&mut self, chars: &[char], start: usize) -> usize {
        match chars.get(start) {
            Some('[') => {
                let mut i = start + 1;
                let mut params = String::new();
                while i < chars.len() && !chars[i].is_ascii_alphabetic() {
                    params.push(chars[i]);
                    i += 1;
                }
                if let Some(&final_byte) = chars.get(i) {
                    self.run_csi(&params, final_byte);
                    i + 1
                } else {
                    i
                }
            }
            Some(']') => {
                let mut i = start + 1;
                while i < chars.len() && chars[i] != '\x07' {
                    if chars[i] == '\x1b' && chars.get(i + 1) == Some(&'\\') {
                        i += 1;
                        break;
                    }
                    i += 1;
                }
                i + 1
            }
            _ => start + 1,
        }
    }

    fn run_csi(&mut self, params: &str, final_byte: char) {
        let count = |p: &str| -> usize {
            let digits: String = p.chars().take_while(|c| c.is_ascii_digit()).collect();
            digits.parse::<usize>().unwrap_or(1)
        };
        match final_byte {
            'A' => {
                self.row = self.row.saturating_sub(count(params));
                self.pending_wrap = false;
            }
            'B' => {
                self.row = (self.row + count(params)).min(self.height - 1);
                self.pending_wrap = false;
            }
            'G' => {
                self.col = (count(params).max(1) - 1).min(self.width - 1);
                self.pending_wrap = false;
            }
            'H' => {
                let mut parts = params.split(';');
                let r = parts.next().and_then(|p| p.parse::<usize>().ok()).unwrap_or(1);
                let c = parts.next().and_then(|p| p.parse::<usize>().ok()).unwrap_or(1);
                self.row = (r.max(1) - 1).min(self.height - 1);
                self.col = (c.max(1) - 1).min(self.width - 1);
                self.pending_wrap = false;
            }
            'J' => {
                if matches!(params.trim(), "2" | "3") {
                    for row in self.grid.iter_mut() {
                        for cell in row.iter_mut() {
                            *cell = ' ';
                        }
                    }
                    for row in self.filled.iter_mut() {
                        for cell in row.iter_mut() {
                            *cell = false;
                        }
                    }
                    self.pending_wrap = false;
                }
            }
            'K' => match params.trim() {
                "0" => {
                    for i in self.col..self.width {
                        self.grid[self.row][i] = ' ';
                        self.filled[self.row][i] = false;
                    }
                }
                "1" => {
                    for i in 0..=self.col {
                        self.grid[self.row][i] = ' ';
                        self.filled[self.row][i] = false;
                    }
                }
                _ => {
                    for i in 0..self.width {
                        self.grid[self.row][i] = ' ';
                        self.filled[self.row][i] = false;
                    }
                }
            },
            'm' => self.set_bg_active(params),
            _ => {}
        }
    }
}

impl TerminalBackend for ScreenBackend {
    fn set_raw_mode(&mut self, _enabled: bool) -> io::Result<()> {
        Ok(())
    }

    fn size(&self) -> io::Result<(u16, u16)> {
        Ok((self.width as u16, self.height as u16))
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn move_up(&mut self, rows: usize) -> io::Result<()> {
        self.run_csi(&rows.to_string(), 'A');
        Ok(())
    }

    fn move_down(&mut self, rows: usize) -> io::Result<()> {
        self.run_csi(&rows.to_string(), 'B');
        Ok(())
    }

    fn move_to_column(&mut self, column: usize) -> io::Result<()> {
        self.run_csi(&(column + 1).to_string(), 'G');
        Ok(())
    }

    fn clear_line(&mut self) -> io::Result<()> {
        self.run_csi("2", 'K');
        Ok(())
    }

    fn write_text(&mut self, text: &str) -> io::Result<()> {
        self.run_text(text);
        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn strip_ansi(line: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\x1b' {
            i = match chars.get(i + 1) {
                Some('[') => {
                    let mut j = i + 2;
                    while j < chars.len() && !chars[j].is_ascii_alphabetic() {
                        j += 1;
                    }
                    j + 1
                }
                Some(']') => {
                    let mut j = i + 2;
                    while j < chars.len() && chars[j] != '\x07' {
                        if chars[j] == '\x1b' && chars.get(j + 1) == Some(&'\\') {
                            j += 1;
                            break;
                        }
                        j += 1;
                    }
                    j + 1
                }
                _ => i + 2,
            };
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// The live region must occupy exactly `rendered.height()` rows ending at the
/// last non-empty screen row, matching the cached layout (ANSI stripped). A
/// wrapped region row shifts the physical rows and desyncs this alignment.
fn assert_screen_region_aligned(controller: &TerminalController<ScreenBackend>) {
    let rendered = controller.rendered().expect("rendered layout").clone();
    let height = rendered.height();
    if height == 0 {
        return;
    }
    let screen = controller.backend.text();
    let last_nonempty = screen
        .iter()
        .rposition(|l| !l.trim().is_empty())
        .expect("nonempty screen");
    let region_top = last_nonempty + 1 - height;
    let expected: Vec<String> = rendered
        .lines
        .iter()
        .map(|l| strip_ansi(l).trim_end().to_string())
        .collect();
    let actual: Vec<String> = screen[region_top..=last_nonempty]
        .iter()
        .map(|l| l.trim_end().to_string())
        .collect();
    assert_eq!(actual, expected, "screen rows diverged from cached layout");
}

mod regressions {
    use super::{ScreenBackend, assert_screen_region_aligned};
    use crate::ui::interactive::controller::TerminalController;
    use crate::ui::interactive::{InteractiveState, ModalOption, ModalState, TranscriptItem};

    fn model_options(count: usize) -> Vec<ModalOption> {
        (0..count)
            .map(|i| {
                ModalOption::new(
                    format!("model-{i}-20260101"),
                    Some(format!("prov\t\t\t{} ctx", 100 + i)),
                )
            })
            .collect()
    }

    fn controller_with_transcript((width, height): (usize, usize)) -> TerminalController<ScreenBackend> {
        let mut controller =
            TerminalController::new(ScreenBackend::new(width, height), InteractiveState::default()).unwrap();
        controller
            .push_transcript_item(TranscriptItem::UserMessage("review the recent changes please".into()))
            .unwrap();
        let filler: Vec<String> = (0..20)
            .map(|i| format!("assistant output line {i} of the answer continues here"))
            .collect();
        controller.write_output(&format!("{}\n", filler.join("\n"))).unwrap();
        controller
    }

    fn open_selector(controller: &mut TerminalController<ScreenBackend>, count: usize) {
        let mut modal = ModalState::new("Select Model", "", model_options(count)).with_search(true);
        modal.selected = 1;
        controller.state_mut().push_modal(modal);
        controller.redraw().unwrap();
        assert_screen_region_aligned(controller);
    }

    fn arrow_down(controller: &mut TerminalController<ScreenBackend>, steps: usize) {
        for _ in 0..steps {
            controller.state_mut().select_next_modal_option();
            controller.redraw().unwrap();
            assert_screen_region_aligned(controller);
            let banner_count = controller
                .backend
                .text()
                .iter()
                .filter(|l| l.contains("Select Model"))
                .count();
            assert_eq!(banner_count, 1, "modal header repeated on screen");
        }
    }

    #[test]
    fn draft_line_wider_than_terminal_keeps_model_selector_repaints_aligned() {
        let mut controller = controller_with_transcript((80, 24));
        controller
            .state_mut()
            .editor_mut()
            .set_text("half written prompt that was never sent to the agent yet");

        open_selector(&mut controller, 37);
        arrow_down(&mut controller, 8);
    }

    #[test]
    fn long_filter_query_keeps_model_selector_repaints_aligned() {
        let mut controller = controller_with_transcript((80, 24));

        open_selector(&mut controller, 37);
        let query: String = "x".repeat(90);
        controller.state_mut().active_modal_mut().unwrap().set_filter(&query);
        controller.redraw().unwrap();
        assert_screen_region_aligned(&controller);

        arrow_down(&mut controller, 6);
    }

    #[test]
    fn long_completion_value_keeps_editor_repaints_aligned() {
        use crate::repl::interactive::Completion;
        let mut controller = controller_with_transcript((80, 24));
        controller.state_mut().editor_mut().set_text("src/ui/int");
        controller.state_mut().autocomplete.open(vec![
            Completion {
                value: "x".repeat(90),
                description: None,
                replacement: 0..10,
            },
            Completion {
                value: "y".repeat(90),
                description: None,
                replacement: 0..10,
            },
        ]);
        controller.redraw().unwrap();
        assert_screen_region_aligned(&controller);

        controller.state_mut().autocomplete.select_next();
        controller.redraw().unwrap();
        assert_screen_region_aligned(&controller);
    }

    mod spacing {
        use super::*;

        /// The committed tool card renders `"\n" + block`, so the standard
        /// separation is exactly one unfilled blank row between the preceding
        /// content and the card's filled top padding.
        fn assert_one_blank_above_committed_card(controller: &TerminalController<ScreenBackend>) {
            let screen = controller.backend.text();
            let header_row = screen
                .iter()
                .position(|l| l.contains("bash cargo test"))
                .expect("committed bash card header");
            let mut row = header_row - 1;
            while row > 0 && screen[row].trim().is_empty() && controller.backend.filled[row].iter().any(|&f| f) {
                row -= 1;
            }
            let mut blank_rows = 0;
            loop {
                let any_fill = controller.backend.filled[row].iter().any(|&f| f);
                if !screen[row].trim().is_empty() || any_fill {
                    break;
                }
                blank_rows += 1;
                if row == 0 {
                    break;
                }
                row -= 1;
            }
            assert_eq!(
                blank_rows,
                1,
                "expected exactly one blank row above the card\n{}",
                controller.backend.dump()
            );
        }

        /// Drives the real presenter (markdown stream + tool events) through the
        /// event channel with the same drain ordering the live batch uses.
        fn drive_renderer_to_controller(
            events: &mut tokio::sync::mpsc::UnboundedReceiver<crate::ui::interactive::UiEvent>,
            controller: &mut TerminalController<ScreenBackend>,
        ) {
            let mut pending = crate::ui::interactive::PendingUiBatch::new(16 * 1024);
            while let Ok(event) = events.try_recv() {
                pending.push(event);
            }
            let drained = pending.drain();
            if let Some(activity) = drained.activity {
                controller.state_mut().footer_mut().activity = activity;
            }
            if let Some(request) = drained.tool_start {
                controller.start_tool(request).unwrap();
            }
            if !drained.tool_chunks.is_empty() {
                controller
                    .append_tool_chunks(drained.tool_chunks.iter().map(String::as_str))
                    .unwrap();
            }
            let had_outputs = !drained.outputs.is_empty();
            for output in drained.outputs {
                match output {
                    crate::ui::interactive::OutputEvent::Text(text) => {
                        controller.write_output(&text).unwrap();
                    }
                    crate::ui::interactive::OutputEvent::StreamText(text) => {
                        controller.write_stream_output(&text).unwrap();
                    }
                }
            }
            let had_transcript = !drained.transcript_items.is_empty();
            for item in drained.transcript_items {
                controller.push_transcript_item(item).unwrap();
            }
            if !had_outputs && !had_transcript {
                controller.redraw().unwrap();
            }
        }

        fn commit_bash_after_stream(
            controller: &mut TerminalController<ScreenBackend>,
            events: &mut tokio::sync::mpsc::UnboundedReceiver<crate::ui::interactive::UiEvent>,
            renderer: &crate::ui::TerminalRenderer,
        ) {
            renderer.start_tool_run("bash", &serde_json::json!({"command": "cargo test"}));
            renderer.tool_chunk("test result: ok");
            renderer.finish_tool_line(rho_harness_core::presentation::ToolLine {
                name: "bash".into(),
                arguments: serde_json::json!({"command": "cargo test"}),
                is_error: false,
                output: "test result: ok".into(),
                output_summary: "1 line".into(),
                duration_ms: Some(1500),
            });
            drive_renderer_to_controller(events, controller);
            controller.redraw().unwrap();
        }

        fn stream_through_presenter(
            controller: &mut TerminalController<ScreenBackend>,
            events: &mut tokio::sync::mpsc::UnboundedReceiver<crate::ui::interactive::UiEvent>,
            renderer: &crate::ui::TerminalRenderer,
            message: &str,
            thinking: &[&str],
        ) {
            // Mirror the approval sink: after thinking or a tool, text gets a prefix blank.
            let mut after_tool_or_thinking = false;
            for token in message.split_inclusive(' ') {
                if after_tool_or_thinking {
                    renderer.write_output("\n");
                    drive_renderer_to_controller(events, controller);
                    after_tool_or_thinking = false;
                }
                renderer.print_token(token);
                drive_renderer_to_controller(events, controller);
            }
            renderer.flush();
            drive_renderer_to_controller(events, controller);

            if !thinking.is_empty() {
                for token in thinking {
                    renderer.print_thinking_token(token);
                    drive_renderer_to_controller(events, controller);
                }
                renderer.write_output("\n");
                drive_renderer_to_controller(events, controller);
            }
        }

        fn real_presenter_scenario(message: &str, thinking: &[&str]) -> TerminalController<ScreenBackend> {
            let (ui, mut events) = crate::ui::interactive::InteractiveUi::channel();
            let renderer = crate::ui::TerminalRenderer::with_ui(ui);
            let mut controller = controller_with_transcript((80, 24));
            controller.state_mut().editor_mut().set_text("");
            stream_through_presenter(&mut controller, &mut events, &renderer, message, thinking);
            commit_bash_after_stream(&mut controller, &mut events, &renderer);
            controller
        }

        #[test]
        fn committed_bash_card_keeps_single_blank_after_paragraph() {
            let controller = real_presenter_scenario("Here is the plan step one.", &[]);
            assert_one_blank_above_committed_card(&controller);
            assert_screen_region_aligned(&controller);
        }

        #[test]
        fn committed_bash_card_keeps_single_blank_after_code_fence() {
            let controller = real_presenter_scenario("Here is the plan:\n```sh\ncargo test\n```\n", &[]);
            assert_one_blank_above_committed_card(&controller);
            assert_screen_region_aligned(&controller);
        }

        #[test]
        fn committed_bash_card_keeps_single_blank_after_trailing_blank_line() {
            let controller = real_presenter_scenario("Step one done.\n\n", &[]);
            assert_one_blank_above_committed_card(&controller);
            assert_screen_region_aligned(&controller);
        }

        #[test]
        fn committed_bash_card_keeps_single_blank_after_thinking() {
            let controller = real_presenter_scenario("", &["Let me check the tests first. "]);
            assert_one_blank_above_committed_card(&controller);
            assert_screen_region_aligned(&controller);
        }

        #[test]
        fn committed_bash_card_keeps_single_blank_after_user_message_and_notice() {
            let mut controller = controller_with_transcript((80, 24));
            controller.state_mut().editor_mut().set_text("");
            controller
                .push_transcript_item(TranscriptItem::UserMessage("run the tests".into()))
                .unwrap();
            controller
                .push_transcript_item(TranscriptItem::Notice("Compaction: 1.2k tokens billed\n".into()))
                .unwrap();
            let (ui, mut events) = crate::ui::interactive::InteractiveUi::channel();
            let renderer = crate::ui::TerminalRenderer::with_ui(ui);
            stream_through_presenter(&mut controller, &mut events, &renderer, "", &[]);
            commit_bash_after_stream(&mut controller, &mut events, &renderer);
            assert_one_blank_above_committed_card(&controller);
            assert_screen_region_aligned(&controller);
        }

        #[test]
        fn committed_bash_card_keeps_single_blank_after_previous_bash_card() {
            let (ui, mut events) = crate::ui::interactive::InteractiveUi::channel();
            let renderer = crate::ui::TerminalRenderer::with_ui(ui);
            let mut controller = controller_with_transcript((80, 24));
            controller.state_mut().editor_mut().set_text("");
            commit_bash_after_stream(&mut controller, &mut events, &renderer);
            commit_bash_after_stream(&mut controller, &mut events, &renderer);
            assert_one_blank_above_committed_card(&controller);
            assert_screen_region_aligned(&controller);
        }

        #[test]
        fn stream_after_tool_preserves_word_without_premature_newline() {
            let (ui, mut events) = crate::ui::interactive::InteractiveUi::channel();
            let renderer = crate::ui::TerminalRenderer::with_ui(ui);
            let mut controller = controller_with_transcript((80, 24));
            controller.state_mut().editor_mut().set_text("");

            commit_bash_after_stream(&mut controller, &mut events, &renderer);

            renderer.write_output("\n");
            renderer.print_token("Com");
            drive_renderer_to_controller(&mut events, &mut controller);

            renderer.print_token("mitted in 78d510a:\n");
            renderer.print_token("• fix(ui): wrap welcome screen items on word boundaries\n");
            renderer.flush();
            drive_renderer_to_controller(&mut events, &mut controller);

            let lines = controller.backend.text();
            assert!(
                lines.iter().any(|l| l.contains("Committed in 78d510a:")),
                "Expected 'Committed in 78d510a:' on a single line, but lines were:\n{lines:#?}"
            );
            assert!(
                !lines.iter().any(|l| l.trim() == "Com"),
                "'Com' was split onto its own line:\n{lines:#?}"
            );
        }

        #[test]
        fn screen_sim_unfocused_working_spinner() {
            let mut controller = controller_with_transcript((80, 24));
            controller.state_mut().footer_mut().activity = crate::ui::interactive::Activity::Working;
            controller.redraw().unwrap();
            let screen_focused = controller.backend.text();

            controller.set_focused(false);
            controller.redraw().unwrap();
            let screen_unfocused = controller.backend.text();

            assert!(screen_focused.iter().any(|l| l.contains("Working...")));
            assert!(screen_unfocused.iter().any(|l| l.contains("Working...")));
        }

        /// The deny-reason input screen: selecting "Deny" (input spec) enters
        /// input mode labeled "reason"; the typed reason must render on a
        /// labeled prompt row, visually separated from the command body.
        #[test]
        fn deny_reason_input_screen() {
            use crate::ui::interactive::InteractionInput;
            let mut controller = controller_with_transcript((80, 24));
            controller.state_mut().editor_mut().set_text("");
            let options = vec![
                ModalOption::new("Allow", Some("Run this tool call once")),
                ModalOption::new("Edit", Some("Edit tool arguments")),
                ModalOption::new("Always", Some("Save rule")),
                ModalOption::new("Deny", Some("Deny tool execution")),
            ];
            let mut modal = ModalState::new(
                "Permission Required",
                "Tool: bash\nInput: cargo test --workspace",
                options,
            )
            .with_option_layout(crate::ui::interactive::OptionLayout::Horizontal);
            modal.selected = 3;
            controller.state_mut().push_modal(modal);
            let spec = InteractionInput {
                label: "reason".into(),
                value: None,
            };
            if let Some(active) = controller.state_mut().active_modal_mut() {
                active.selected = 3;
                active.input_option = Some(3);
                active.enter_input_mode(&spec.label);
                for c in "tests are flaky".chars() {
                    active.input.insert(c);
                }
            }
            controller.redraw().unwrap();
            super::assert_screen_region_aligned(&controller);
            println!("=== deny reason input screen ===\n{}", controller.backend.dump());
            let screen = controller.backend.text();
            let input_row = screen
                .iter()
                .find(|l| l.contains("tests are flaky"))
                .expect("reason row");
            assert!(
                input_row.contains("reason"),
                "reason row must carry its label: {input_row:?}"
            );
            assert!(
                input_row.contains('\u{203a}'),
                "reason row must carry the prompt marker: {input_row:?}"
            );
            assert!(
                screen.iter().any(|l| l.contains("Shift+Enter newline")),
                "hint must surface the newline key"
            );
        }
    }
}
