#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyAction {
    AppInterrupt,
    AppClear,
    AppExit,
    AppSuspend,
    AppEditorExternal,
    AppClipboardPasteImage,
    AppModelSelect,
    AppModelCycleForward,
    AppModelCycleBackward,
    AppThinkingCycle,
    AppThinkingToggle,
    AppToolsExpand,
    AppMessageCopy,
    AppMessageFollowUp,
    AppMessageDequeue,
    AppSessionNew,
    AppSessionTree,
    AppSessionFork,
    AppSessionResume,
    TuiEditorCursorUp,
    TuiEditorCursorDown,
    TuiEditorCursorLeft,
    TuiEditorCursorRight,
    TuiEditorCursorWordLeft,
    TuiEditorCursorWordRight,
    TuiEditorCursorLineStart,
    TuiEditorCursorLineEnd,
    TuiEditorDeleteCharBackward,
    TuiEditorDeleteCharForward,
    TuiEditorDeleteWordBackward,
    TuiEditorDeleteWordForward,
    TuiEditorDeleteToLineStart,
    TuiEditorDeleteToLineEnd,
    TuiEditorYank,
    TuiEditorUndo,
    TuiInputNewLine,
    TuiInputSubmit,
    TuiInputTab,
    TuiSelectUp,
    TuiSelectDown,
    TuiSelectConfirm,
    TuiSelectCancel,
}

const ACTION_MAP: &[(KeyAction, &str)] = &[
    (KeyAction::AppInterrupt, "app.interrupt"),
    (KeyAction::AppClear, "app.clear"),
    (KeyAction::AppExit, "app.exit"),
    (KeyAction::AppSuspend, "app.suspend"),
    (KeyAction::AppEditorExternal, "app.editor.external"),
    (KeyAction::AppClipboardPasteImage, "app.clipboard.pasteImage"),
    (KeyAction::AppModelSelect, "app.model.select"),
    (KeyAction::AppModelCycleForward, "app.model.cycleForward"),
    (KeyAction::AppModelCycleBackward, "app.model.cycleBackward"),
    (KeyAction::AppThinkingCycle, "app.thinking.cycle"),
    (KeyAction::AppThinkingToggle, "app.thinking.toggle"),
    (KeyAction::AppToolsExpand, "app.tools.expand"),
    (KeyAction::AppMessageCopy, "app.message.copy"),
    (KeyAction::AppMessageFollowUp, "app.message.followUp"),
    (KeyAction::AppMessageDequeue, "app.message.dequeue"),
    (KeyAction::AppSessionNew, "app.session.new"),
    (KeyAction::AppSessionTree, "app.session.tree"),
    (KeyAction::AppSessionFork, "app.session.fork"),
    (KeyAction::AppSessionResume, "app.session.resume"),
    (KeyAction::TuiEditorCursorUp, "tui.editor.cursorUp"),
    (KeyAction::TuiEditorCursorDown, "tui.editor.cursorDown"),
    (KeyAction::TuiEditorCursorLeft, "tui.editor.cursorLeft"),
    (KeyAction::TuiEditorCursorRight, "tui.editor.cursorRight"),
    (KeyAction::TuiEditorCursorWordLeft, "tui.editor.cursorWordLeft"),
    (KeyAction::TuiEditorCursorWordRight, "tui.editor.cursorWordRight"),
    (KeyAction::TuiEditorCursorLineStart, "tui.editor.cursorLineStart"),
    (KeyAction::TuiEditorCursorLineEnd, "tui.editor.cursorLineEnd"),
    (KeyAction::TuiEditorDeleteCharBackward, "tui.editor.deleteCharBackward"),
    (KeyAction::TuiEditorDeleteCharForward, "tui.editor.deleteCharForward"),
    (KeyAction::TuiEditorDeleteWordBackward, "tui.editor.deleteWordBackward"),
    (KeyAction::TuiEditorDeleteWordForward, "tui.editor.deleteWordForward"),
    (KeyAction::TuiEditorDeleteToLineStart, "tui.editor.deleteToLineStart"),
    (KeyAction::TuiEditorDeleteToLineEnd, "tui.editor.deleteToLineEnd"),
    (KeyAction::TuiEditorYank, "tui.editor.yank"),
    (KeyAction::TuiEditorUndo, "tui.editor.undo"),
    (KeyAction::TuiInputNewLine, "tui.input.newLine"),
    (KeyAction::TuiInputSubmit, "tui.input.submit"),
    (KeyAction::TuiInputTab, "tui.input.tab"),
    (KeyAction::TuiSelectUp, "tui.select.up"),
    (KeyAction::TuiSelectDown, "tui.select.down"),
    (KeyAction::TuiSelectConfirm, "tui.select.confirm"),
    (KeyAction::TuiSelectCancel, "tui.select.cancel"),
];

impl KeyAction {
    pub fn as_str(self) -> &'static str {
        ACTION_MAP
            .iter()
            .find(|(action, _)| *action == self)
            .map(|(_, name)| *name)
            .unwrap_or("")
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::from_name(id)
    }

    pub fn from_name(name: &str) -> Option<Self> {
        ACTION_MAP.iter().find(|(_, n)| *n == name).map(|(action, _)| *action)
    }
}
