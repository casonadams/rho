use super::binding::{BindingError, BoundString, apply_data_patch, resolve_pointer, resolve_string};
use super::components::A2uiComponent;
use super::protocol::{
    A2uiMessage, CreateSurface, DEFAULT_CATALOG_ID, DeleteSurface, SurfacePlacement, UpdateComponents, UpdateDataModel,
};
use super::{MAX_COMPONENTS, MAX_DEPTH, MAX_SURFACES};
use serde_json::{Map, Value};
use std::cmp::Reverse;
use std::collections::{BTreeMap, HashSet};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum A2uiError {
    #[error("maximum active A2UI surfaces ({max}) exceeded")]
    SurfaceLimitExceeded { max: usize },
    #[error("surface '{surface_id}' exceeded maximum components ({max})")]
    ComponentLimitExceeded { surface_id: String, max: usize },
    #[error("A2UI surface '{surface_id}' does not exist")]
    SurfaceNotFound { surface_id: String },
    #[error(transparent)]
    Binding(#[from] BindingError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisitAction {
    Continue,
    SkipChildren,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct A2uiSurface {
    pub surface_id: String,
    pub catalog_id: String,
    pub placement: SurfacePlacement,
    pub title: Option<String>,
    pub priority: i32,
    pub root_id: String,
    pub components: BTreeMap<String, A2uiComponent>,
    pub data_model: Value,
    pub(crate) sequence: u64,
}

impl A2uiSurface {
    pub fn component(&self, id: &str) -> Option<&A2uiComponent> {
        self.components.get(id)
    }

    pub fn resolve_pointer(&self, pointer: &str) -> Option<&Value> {
        resolve_pointer(&self.data_model, pointer)
    }

    pub fn resolve_string(&self, bound: &BoundString) -> String {
        resolve_string(&self.data_model, bound)
    }

    /// Walks the component tree starting at `root_id`, enforcing `MAX_DEPTH` and
    /// cycle protection via a visited-ID set.
    pub fn walk_tree<'a>(&'a self, mut visit: impl FnMut(&'a A2uiComponent, usize) -> VisitAction) {
        let mut visited = HashSet::new();
        self.walk_node(&self.root_id, 0, &mut visited, &mut visit);
    }

    fn walk_node<'a>(
        &'a self,
        id: &'a str,
        depth: usize,
        visited: &mut HashSet<&'a str>,
        visit: &mut impl FnMut(&'a A2uiComponent, usize) -> VisitAction,
    ) {
        if depth >= MAX_DEPTH || !visited.insert(id) {
            return;
        }
        let Some(component) = self.components.get(id) else {
            return;
        };
        if visit(component, depth) == VisitAction::SkipChildren {
            return;
        }
        for child_id in component.kind.child_ids() {
            self.walk_node(child_id, depth + 1, visited, visit);
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SurfaceStore {
    surfaces: BTreeMap<String, A2uiSurface>,
    next_sequence: u64,
}

impl SurfaceStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.surfaces.is_empty()
    }

    pub fn len(&self) -> usize {
        self.surfaces.len()
    }

    pub fn surface(&self, id: &str) -> Option<&A2uiSurface> {
        self.surfaces.get(id)
    }

    /// Returns active surfaces for `placement`, ordered by descending `priority`,
    /// then insertion order, then `surface_id`.
    pub fn surfaces_for(&self, placement: SurfacePlacement) -> Vec<&A2uiSurface> {
        let mut matching: Vec<&A2uiSurface> = self
            .surfaces
            .values()
            .filter(|surface| surface.placement == placement)
            .collect();
        matching.sort_by_key(|surface| (Reverse(surface.priority), surface.sequence, surface.surface_id.as_str()));
        matching
    }

    pub fn apply_all(&mut self, messages: impl IntoIterator<Item = A2uiMessage>) -> Result<(), A2uiError> {
        for message in messages {
            self.apply(message)?;
        }
        Ok(())
    }

    pub fn apply(&mut self, msg: A2uiMessage) -> Result<(), A2uiError> {
        match msg {
            A2uiMessage::CreateSurface(create) => self.apply_create(create),
            A2uiMessage::UpdateComponents(update) => self.apply_components(update),
            A2uiMessage::UpdateDataModel(patch) => self.apply_data(patch),
            A2uiMessage::DeleteSurface(delete) => self.apply_delete(delete),
        }
    }

    fn apply_create(&mut self, create: CreateSurface) -> Result<(), A2uiError> {
        if let Some(existing) = self.surfaces.get_mut(&create.surface_id) {
            existing.catalog_id = create.catalog_id.unwrap_or_else(|| DEFAULT_CATALOG_ID.to_string());
            existing.placement = create.placement;
            existing.title = create.title;
            existing.priority = create.priority;
            existing.root_id = create.root_id;
            return Ok(());
        }

        if self.surfaces.len() >= MAX_SURFACES {
            return Err(A2uiError::SurfaceLimitExceeded { max: MAX_SURFACES });
        }

        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        let surface_id = create.surface_id;
        self.surfaces.insert(
            surface_id.clone(),
            A2uiSurface {
                surface_id,
                catalog_id: create.catalog_id.unwrap_or_else(|| DEFAULT_CATALOG_ID.to_string()),
                placement: create.placement,
                title: create.title,
                priority: create.priority,
                root_id: create.root_id,
                components: BTreeMap::new(),
                data_model: Value::Object(Map::new()),
                sequence,
            },
        );
        Ok(())
    }

    fn apply_components(&mut self, update: UpdateComponents) -> Result<(), A2uiError> {
        let Some(surface) = self.surfaces.get_mut(&update.surface_id) else {
            return Err(A2uiError::SurfaceNotFound {
                surface_id: update.surface_id,
            });
        };

        let mut pending_new_ids = HashSet::new();
        for component in &update.components {
            if !surface.components.contains_key(&component.id) {
                pending_new_ids.insert(component.id.as_str());
            }
        }
        if surface.components.len() + pending_new_ids.len() > MAX_COMPONENTS {
            return Err(A2uiError::ComponentLimitExceeded {
                surface_id: update.surface_id,
                max: MAX_COMPONENTS,
            });
        }

        for component in update.components {
            surface.components.insert(component.id.clone(), component);
        }
        Ok(())
    }

    fn apply_data(&mut self, patch: UpdateDataModel) -> Result<(), A2uiError> {
        let Some(surface) = self.surfaces.get_mut(&patch.surface_id) else {
            return Err(A2uiError::SurfaceNotFound {
                surface_id: patch.surface_id,
            });
        };
        apply_data_patch(&mut surface.data_model, patch.path.as_deref(), patch.value)?;
        Ok(())
    }

    fn apply_delete(&mut self, delete: DeleteSurface) -> Result<(), A2uiError> {
        self.surfaces.remove(&delete.surface_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::a2ui::components::{A2uiComponentKind, TextUsageHint};
    use serde_json::json;

    #[test]
    fn store_orders_surfaces_by_priority_descending_then_insertion_order() {
        let mut store = SurfaceStore::new();
        for (id, priority) in [("low", 5), ("high", 20), ("mid_a", 10), ("mid_b", 10)] {
            store
                .apply(A2uiMessage::CreateSurface(CreateSurface {
                    surface_id: id.to_string(),
                    catalog_id: None,
                    placement: SurfacePlacement::AboveEditor,
                    title: None,
                    priority,
                    root_id: "root".to_string(),
                }))
                .unwrap();
        }
        store
            .apply(A2uiMessage::CreateSurface(CreateSurface {
                surface_id: "below".to_string(),
                catalog_id: None,
                placement: SurfacePlacement::BelowEditor,
                title: None,
                priority: 100,
                root_id: "root".to_string(),
            }))
            .unwrap();

        let above_ids: Vec<&str> = store
            .surfaces_for(SurfacePlacement::AboveEditor)
            .into_iter()
            .map(|s| s.surface_id.as_str())
            .collect();
        assert_eq!(above_ids, vec!["high", "mid_a", "mid_b", "low"]);

        let below_ids: Vec<&str> = store
            .surfaces_for(SurfacePlacement::BelowEditor)
            .into_iter()
            .map(|s| s.surface_id.as_str())
            .collect();
        assert_eq!(below_ids, vec!["below"]);

        store
            .apply(A2uiMessage::DeleteSurface(DeleteSurface {
                surface_id: "high".to_string(),
            }))
            .unwrap();
        assert!(store.surface("high").is_none());
    }

    #[test]
    fn store_enforces_surface_and_component_caps() {
        let mut store = SurfaceStore::new();
        for idx in 0..MAX_SURFACES {
            store
                .apply(A2uiMessage::CreateSurface(CreateSurface::new(format!("s{idx}"))))
                .unwrap();
        }
        let err = store
            .apply(A2uiMessage::CreateSurface(CreateSurface::new("overflow")))
            .unwrap_err();
        assert!(matches!(err, A2uiError::SurfaceLimitExceeded { max: MAX_SURFACES }));

        let too_many: Vec<A2uiComponent> = (0..=MAX_COMPONENTS)
            .map(|idx| {
                A2uiComponent::new(
                    format!("c{idx}"),
                    A2uiComponentKind::Text {
                        text: BoundString::literal("x"),
                        usage_hint: TextUsageHint::Body,
                        tone: None,
                    },
                )
            })
            .collect();
        let err = store
            .apply(A2uiMessage::UpdateComponents(UpdateComponents {
                surface_id: "s0".to_string(),
                components: too_many,
            }))
            .unwrap_err();
        assert!(matches!(
            err,
            A2uiError::ComponentLimitExceeded {
                max: MAX_COMPONENTS,
                ..
            }
        ));
    }

    #[test]
    fn walk_tree_handles_cycles_and_enforces_max_depth() {
        let mut store = SurfaceStore::new();
        store
            .apply(A2uiMessage::CreateSurface(CreateSurface::new("cyclic")))
            .unwrap();
        store
            .apply(A2uiMessage::UpdateComponents(UpdateComponents {
                surface_id: "cyclic".to_string(),
                components: vec![
                    A2uiComponent::new(
                        "root",
                        A2uiComponentKind::Column {
                            children: vec!["a".to_string()],
                            gap: None,
                        },
                    ),
                    A2uiComponent::new(
                        "a",
                        A2uiComponentKind::Column {
                            children: vec!["root".to_string()],
                            gap: None,
                        },
                    ),
                ],
            }))
            .unwrap();

        let surface = store.surface("cyclic").unwrap();
        let mut visited = Vec::new();
        surface.walk_tree(|comp, depth| {
            visited.push((comp.id.as_str(), depth));
            VisitAction::Continue
        });
        assert_eq!(visited, vec![("root", 0), ("a", 1)]);

        // Deep chain exceeding MAX_DEPTH
        store
            .apply(A2uiMessage::CreateSurface(CreateSurface::new("deep")))
            .unwrap();
        let mut deep_components = Vec::new();
        for idx in 0..=MAX_DEPTH + 4 {
            let id = if idx == 0 {
                "root".to_string()
            } else {
                format!("n{idx}")
            };
            let next = format!("n{}", idx + 1);
            deep_components.push(A2uiComponent::new(
                id,
                A2uiComponentKind::Column {
                    children: vec![next],
                    gap: None,
                },
            ));
        }
        store
            .apply(A2uiMessage::UpdateComponents(UpdateComponents {
                surface_id: "deep".to_string(),
                components: deep_components,
            }))
            .unwrap();

        let deep_surface = store.surface("deep").unwrap();
        let mut max_seen_depth = 0;
        let mut count = 0;
        deep_surface.walk_tree(|_, depth| {
            max_seen_depth = max_seen_depth.max(depth);
            count += 1;
            VisitAction::Continue
        });
        assert_eq!(count, MAX_DEPTH);
        assert_eq!(max_seen_depth, MAX_DEPTH - 1);
    }

    #[test]
    fn store_applies_data_model_patches() {
        let mut store = SurfaceStore::new();
        store
            .apply(A2uiMessage::CreateSurface(CreateSurface::new("data")))
            .unwrap();
        store
            .apply(A2uiMessage::UpdateDataModel(UpdateDataModel {
                surface_id: "data".to_string(),
                path: Some("/stats/done".to_string()),
                value: json!(3),
            }))
            .unwrap();

        let surface = store.surface("data").unwrap();
        assert_eq!(surface.resolve_string(&BoundString::path("/stats/done")), "3");
    }
}
