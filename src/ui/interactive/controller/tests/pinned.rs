//! A controller pinned to the bottom keeps the live region on the screen's last rows: under blank
//! rows while the output is short, directly under the newest output once it fills the screen.

use super::screen_sim::ScreenBackend;
use crate::ui::interactive::controller::TerminalController;
use crate::ui::interactive::{InteractiveState, ModalOption, ModalState, TranscriptItem};

fn pinned(width: usize, height: usize) -> TerminalController<ScreenBackend> {
    TerminalController::pinned_to_bottom(ScreenBackend::new(width, height), InteractiveState::default()).unwrap()
}

fn print_lines(controller: &mut TerminalController<ScreenBackend>, range: std::ops::Range<usize>) {
    for n in range {
        controller.write_output(&format!("line {n}\n")).unwrap();
    }
}

fn open_menu(controller: &mut TerminalController<ScreenBackend>, options: usize) {
    let options = (0..options)
        .map(|n| ModalOption::new(format!("option-{n}"), Some("description")))
        .collect();
    controller
        .state_mut()
        .push_modal(ModalState::new("Choose", "", options));
    controller.redraw().unwrap();
}

fn close_menu(controller: &mut TerminalController<ScreenBackend>) {
    controller.state_mut().pop_modal();
    controller.redraw().unwrap();
}

fn region_height(controller: &TerminalController<ScreenBackend>) -> usize {
    controller.rendered().expect("a drawn region").height()
}

/// What the screen must show: the region on the last rows, and above it the newest `expected`
/// rows (all of them, from the top, while they fit) with no blank row between them and the region.
fn assert_pinned(controller: &TerminalController<ScreenBackend>, expected: &[String]) {
    let screen = controller.backend.text();
    let height = screen.len();
    let region = region_height(controller);
    let above = height - region;
    assert!(
        !screen[height - 1].trim().is_empty(),
        "the footer is not on the last row\n{}",
        controller.backend.dump()
    );
    let shown: Vec<String> = screen[..above].to_vec();
    let wanted: Vec<String> = if expected.len() >= above {
        expected[expected.len() - above..].to_vec()
    } else {
        let mut rows = expected.to_vec();
        rows.resize(above, String::new());
        rows
    };
    assert_eq!(shown, wanted, "rows above the region\n{}", controller.backend.dump());
}

fn rows_of(range: std::ops::Range<usize>) -> Vec<String> {
    range.map(|n| format!("line {n}")).collect()
}

#[test]
fn a_fresh_screen_draws_the_region_on_the_last_rows_under_blank_space() {
    let controller = pinned(70, 20);
    assert_pinned(&controller, &[]);
    assert!(
        controller.backend.text()[..20 - region_height(&controller)]
            .iter()
            .all(String::is_empty)
    );
}

#[test]
fn short_output_stays_at_the_top_with_the_region_still_on_the_last_rows() {
    let mut controller = pinned(70, 20);
    print_lines(&mut controller, 0..3);
    assert_pinned(&controller, &rows_of(0..3));
    assert_eq!(controller.backend.text()[0], "line 0");
}

#[test]
fn a_menu_opening_and_closing_over_short_output_never_moves_the_footer_or_scrolls() {
    let mut controller = pinned(70, 20);
    print_lines(&mut controller, 0..3);
    let scrolled = controller.backend.scrollback.len();

    open_menu(&mut controller, 6);
    assert_pinned(&controller, &rows_of(0..3));
    close_menu(&mut controller);
    assert_pinned(&controller, &rows_of(0..3));
    assert_eq!(controller.backend.scrollback.len(), scrolled);
}

#[test]
fn output_filling_the_screen_scrolls_with_the_region_directly_under_it() {
    let mut controller = pinned(70, 20);
    print_lines(&mut controller, 0..40);
    assert_pinned(&controller, &rows_of(0..40));
}

#[test]
fn a_menu_closing_over_a_full_screen_brings_back_the_rows_it_covered() {
    let mut controller = pinned(70, 20);
    print_lines(&mut controller, 0..40);
    open_menu(&mut controller, 8);
    assert_pinned(&controller, &rows_of(0..40));
    close_menu(&mut controller);
    assert_pinned(&controller, &rows_of(0..40));
}

#[test]
fn output_after_a_menu_closed_continues_directly_under_the_rows() {
    let mut controller = pinned(70, 20);
    print_lines(&mut controller, 0..40);
    open_menu(&mut controller, 8);
    close_menu(&mut controller);
    print_lines(&mut controller, 40..43);
    assert_pinned(&controller, &rows_of(0..43));
}

#[test]
fn a_line_still_being_streamed_stays_one_row_and_keeps_the_region_pinned() {
    let mut controller = pinned(70, 20);
    controller.write_output("done\n").unwrap();
    controller.write_stream_output("partial").unwrap();
    assert_pinned(&controller, &["done".into(), "partial".into()]);
    controller.write_stream_output(" and the rest\n").unwrap();
    assert_pinned(&controller, &["done".into(), "partial and the rest".into()]);
}

#[test]
fn a_resize_repins_the_region_to_the_new_last_row() {
    let mut controller = pinned(70, 20);
    controller
        .push_transcript_item(TranscriptItem::UserMessage("hello there".into()))
        .unwrap();
    controller.backend.resize(60, 30);
    controller.resize_to(60, 30).unwrap();
    let screen = controller.backend.text();
    assert!(!screen[29].trim().is_empty(), "{}", controller.backend.dump());
    assert!(screen.iter().any(|row| row.contains("hello there")));
}

#[test]
fn a_growing_editor_takes_the_blank_rows_and_a_shrinking_one_gives_them_back() {
    let mut controller = pinned(70, 20);
    print_lines(&mut controller, 0..3);
    controller.state_mut().editor_mut().set_text("one\ntwo\nthree\nfour");
    controller.redraw().unwrap();
    assert_pinned(&controller, &rows_of(0..3));
    controller.state_mut().editor_mut().set_text("");
    controller.redraw().unwrap();
    assert_pinned(&controller, &rows_of(0..3));
}

/// A deterministic mix of output, wrapped output, menus and editor growth: after every step the
/// screen must show the newest printed rows above a region on the last row.
#[test]
fn any_mix_of_output_menus_and_editing_keeps_the_region_pinned_with_no_gap_over_hidden_rows() {
    let width = 40;
    let mut controller = pinned(width, 18);
    let mut expected: Vec<String> = Vec::new();
    let mut seed = 0x9e37_79b9_u32;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed as usize
    };
    let mut menu_open = false;
    for step in 0..400 {
        match next() % 6 {
            0..=2 => {
                let length = 1 + next() % (width * 2 + 10);
                let text: String = (0..length)
                    .map(|i| char::from(b'a' + ((i + step) % 26) as u8))
                    .collect();
                controller.write_output(&format!("{text}\n")).unwrap();
                let letters: Vec<char> = text.chars().collect();
                expected.extend(letters.chunks(width).map(|chunk| chunk.iter().collect::<String>()));
            }
            3 if !menu_open => {
                open_menu(&mut controller, 1 + next() % 9);
                menu_open = true;
            }
            4 if menu_open => {
                close_menu(&mut controller);
                menu_open = false;
            }
            _ if !menu_open => {
                let lines = next() % 4;
                controller
                    .state_mut()
                    .editor_mut()
                    .set_text(vec!["draft"; lines].join("\n"));
                controller.redraw().unwrap();
            }
            _ => {}
        }
        assert_pinned(&controller, &expected);
    }
}
