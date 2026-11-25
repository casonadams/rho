//! A2UI v1.0 declarative surface protocol, RFC 6901 JSON-Pointer bindings,
//! surface state store, and pure built-in surface builders.

pub mod binding;
pub mod components;
pub mod protocol;
pub mod store;
pub mod surfaces;

pub const MAX_SURFACES: usize = 16;
pub const MAX_COMPONENTS: usize = 256;
pub const MAX_DEPTH: usize = 16;

pub use binding::{
    BindingError, BoundString, BoundValue, apply_data_patch, resolve_pointer, resolve_string, resolve_typed,
};
pub use components::{
    A2uiComponent, A2uiComponentKind, ActionBinding, BoundTableRows, ButtonVariant, ChoiceOption, RowJustify,
    SemanticTone, TextUsageHint,
};
pub use protocol::{
    A2uiMessage, A2uiModalRequest, A2uiModalResult, A2uiUserAction, CreateSurface, DEFAULT_CATALOG_ID, DEFAULT_ROOT_ID,
    DeleteSurface, SurfacePlacement, UpdateComponents, UpdateDataModel,
};
pub use store::{A2uiError, A2uiSurface, SurfaceStore, VisitAction};
pub use surfaces::{
    SUBAGENT_SURFACE_ID, SubagentSurfaceRow, TODO_SURFACE_ID, subagent_surface_messages, todo_surface_messages,
};
