use crate::ui::interactive::{TranscriptItem, TranscriptRenderInput, render_transcript_item};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderSlot {
    Standard,
    Alternate,
}

pub fn is_invariant(item: &TranscriptItem) -> bool {
    matches!(
        item,
        TranscriptItem::Welcome(_)
            | TranscriptItem::UserMessage(_)
            | TranscriptItem::AssistantText(_)
            | TranscriptItem::Notice(_)
    )
}

pub fn is_dual_state(item: &TranscriptItem) -> bool {
    !is_invariant(item)
}

pub fn target_slot(item: &TranscriptItem, tools_expanded: bool, hide_thinking: bool) -> RenderSlot {
    if is_dual_state(item) {
        match item {
            TranscriptItem::Tool(_) if tools_expanded => RenderSlot::Alternate,
            TranscriptItem::Thinking(_) if hide_thinking => RenderSlot::Alternate,
            _ => RenderSlot::Standard,
        }
    } else {
        RenderSlot::Standard
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CachedItemRender {
    pub standard: Option<String>,
    pub alternate: Option<String>,
}

impl CachedItemRender {
    pub fn new(standard: Option<String>, alternate: Option<String>) -> Self {
        Self { standard, alternate }
    }

    pub fn standard(rendered: impl Into<String>) -> Self {
        Self {
            standard: Some(rendered.into()),
            alternate: None,
        }
    }

    pub fn alternate(rendered: impl Into<String>) -> Self {
        Self {
            standard: None,
            alternate: Some(rendered.into()),
        }
    }

    pub fn get(&self, slot: RenderSlot) -> Option<&str> {
        match slot {
            RenderSlot::Standard => self.standard.as_deref(),
            RenderSlot::Alternate => self.alternate.as_deref(),
        }
    }

    pub fn set(&mut self, slot: RenderSlot, rendered: impl Into<String>) {
        let value = Some(rendered.into());
        match slot {
            RenderSlot::Standard => self.standard = value,
            RenderSlot::Alternate => self.alternate = value,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TranscriptRenderCache {
    items: Vec<CachedItemRender>,
}

impl TranscriptRenderCache {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            items: Vec::with_capacity(capacity),
        }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    pub fn entries(&self) -> &[CachedItemRender] {
        &self.items
    }

    pub fn entry(&self, index: usize) -> Option<&CachedItemRender> {
        self.items.get(index)
    }

    pub fn entry_mut(&mut self, index: usize) -> Option<&mut CachedItemRender> {
        self.items.get_mut(index)
    }

    pub fn push(&mut self, slot: RenderSlot, rendered: impl Into<String>) {
        let mut entry = CachedItemRender::default();
        entry.set(slot, rendered);
        self.items.push(entry);
    }

    pub fn set(&mut self, index: usize, entry: CachedItemRender) {
        if index >= self.items.len() {
            self.items.resize_with(index + 1, CachedItemRender::default);
        }
        self.items[index] = entry;
    }

    pub fn get(&self, index: usize, slot: RenderSlot) -> Option<&str> {
        self.items.get(index).and_then(|entry| entry.get(slot))
    }

    pub fn get_or_render(&mut self, index: usize, input: TranscriptRenderInput<'_>) -> &str {
        if index >= self.items.len() {
            self.items.resize_with(index + 1, CachedItemRender::default);
        }

        let slot = target_slot(input.item, input.tools_expanded, input.hide_thinking);
        if self.items[index].get(slot).is_none() {
            let rendered = render_transcript_item(input);
            self.items[index].set(slot, rendered);
        }

        self.items[index].get(slot).unwrap_or("")
    }
}
