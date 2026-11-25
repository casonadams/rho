use super::binding::{BoundString, BoundValue};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Semantic color/style roles mapped by the UI layer to `rho`'s ANSI-16 `Theme`.
///
/// The A2UI protocol intentionally forbids hardcoded RGB or hex colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SemanticTone {
    #[default]
    Neutral,
    Accent,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TextUsageHint {
    H1,
    H2,
    H3,
    #[default]
    Body,
    Caption,
    Muted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RowJustify {
    #[default]
    Start,
    Center,
    End,
    SpaceBetween,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ButtonVariant {
    Primary,
    #[default]
    Secondary,
    Danger,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChoiceOption {
    pub value: String,
    pub label: BoundString,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<BoundString>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionBinding {
    pub name: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub context: BTreeMap<String, BoundValue<Value>>,
}

pub type BoundTableRows = BoundValue<Vec<Vec<String>>>;

/// A node in a surface's flat adjacency-list component tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct A2uiComponent {
    pub id: String,
    #[serde(flatten)]
    pub kind: A2uiComponentKind,
}

impl A2uiComponent {
    pub fn new(id: impl Into<String>, kind: A2uiComponentKind) -> Self {
        Self { id: id.into(), kind }
    }
}

/// Declarative A2UI v1.0 component catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "component", rename_all_fields = "camelCase")]
pub enum A2uiComponentKind {
    Column {
        children: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        gap: Option<u8>,
    },
    Row {
        children: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        gap: Option<u8>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        justify: Option<RowJustify>,
    },
    Card {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<BoundString>,
        child: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tone: Option<SemanticTone>,
    },
    Divider {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<BoundString>,
    },
    Text {
        text: BoundString,
        #[serde(default)]
        usage_hint: TextUsageHint,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tone: Option<SemanticTone>,
    },
    Code {
        code: BoundString,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        language: Option<String>,
    },
    Badge {
        label: BoundString,
        #[serde(default)]
        tone: SemanticTone,
    },
    Table {
        headers: Vec<BoundString>,
        rows: BoundTableRows,
    },
    Button {
        label: BoundString,
        action: ActionBinding,
        #[serde(default)]
        variant: ButtonVariant,
    },
    ChoicePicker {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<BoundString>,
        options: Vec<ChoiceOption>,
        selection_path: String,
        #[serde(default)]
        multi_select: bool,
        #[serde(default)]
        allow_other: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        other_path: Option<String>,
    },
    TextField {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<BoundString>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        placeholder: Option<String>,
        value_path: String,
        #[serde(default)]
        multiline: bool,
        #[serde(default)]
        masked: bool,
    },
    CheckBox {
        label: BoundString,
        checked_path: String,
    },
}

impl A2uiComponentKind {
    /// Returns the IDs of any direct child components referenced by this component.
    pub fn child_ids(&self) -> Vec<&str> {
        match self {
            Self::Column { children, .. } | Self::Row { children, .. } => children.iter().map(String::as_str).collect(),
            Self::Card { child, .. } => vec![child.as_str()],
            Self::Divider { .. }
            | Self::Text { .. }
            | Self::Code { .. }
            | Self::Badge { .. }
            | Self::Table { .. }
            | Self::Button { .. }
            | Self::ChoicePicker { .. }
            | Self::TextField { .. }
            | Self::CheckBox { .. } => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn component_serde_round_trip_uses_camel_case_fields() {
        let component = A2uiComponent::new(
            "picker",
            A2uiComponentKind::ChoicePicker {
                label: Some(BoundString::literal("Choose DB")),
                options: vec![ChoiceOption {
                    value: "pg".to_string(),
                    label: BoundString::literal("PostgreSQL"),
                    description: Some(BoundString::path("/pgDesc")),
                }],
                selection_path: "/answers/db".to_string(),
                multi_select: false,
                allow_other: true,
                other_path: Some("/answers/dbOther".to_string()),
            },
        );

        let json = serde_json::to_value(&component).unwrap();
        assert_eq!(json["component"], "ChoicePicker");
        assert_eq!(json["selectionPath"], "/answers/db");
        assert_eq!(json["allowOther"], true);

        let decoded: A2uiComponent = serde_json::from_value(json).unwrap();
        assert_eq!(decoded, component);
    }
}
