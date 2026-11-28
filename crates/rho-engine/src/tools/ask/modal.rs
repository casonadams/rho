use super::{ASK_SURFACE_ID, PreparedQuestion};
use rho_harness_core::a2ui::{
    A2uiComponent, A2uiComponentKind, A2uiMessage, A2uiModalRequest, ActionBinding, BoundString, ButtonVariant,
    ChoiceOption, CreateSurface, RowJustify, SemanticTone, SurfacePlacement, TextUsageHint, UpdateComponents,
    UpdateDataModel,
};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

pub(super) fn build_modal_request(questions: &[PreparedQuestion]) -> A2uiModalRequest {
    let title = modal_title(questions);
    let mut components = Vec::new();
    let mut root_children = Vec::new();
    let mut initial_answers = Map::new();

    for q in questions {
        root_children.push(push_question(&mut components, q));
        initial_answers.insert(q.key.clone(), initial_answer_value(q));
    }

    append_action_buttons(&mut components, &mut root_children);
    components.push(A2uiComponent::new(
        "root",
        A2uiComponentKind::Column {
            children: root_children,
            gap: Some(1),
        },
    ));

    let messages = vec![
        A2uiMessage::CreateSurface(CreateSurface {
            surface_id: ASK_SURFACE_ID.to_string(),
            catalog_id: None,
            placement: SurfacePlacement::Modal,
            title: Some(title.clone()),
            priority: 100,
            root_id: "root".to_string(),
        }),
        A2uiMessage::UpdateComponents(UpdateComponents {
            surface_id: ASK_SURFACE_ID.to_string(),
            components,
        }),
        A2uiMessage::UpdateDataModel(UpdateDataModel {
            surface_id: ASK_SURFACE_ID.to_string(),
            path: None,
            value: json!({ "answers": initial_answers }),
        }),
    ];

    A2uiModalRequest {
        surface_id: ASK_SURFACE_ID.to_string(),
        title: Some(title),
        messages,
        timeout_ms: None,
    }
}

/// Adds `q`'s field (and its header, grouped with it) to `components`; returns the id the root lists.
fn push_question(components: &mut Vec<A2uiComponent>, q: &PreparedQuestion) -> String {
    let field_id = format!("q_{}", q.key);
    components.push(build_question_component(&field_id, q));
    let Some(header) = &q.redacted_header else {
        return field_id;
    };
    let header_id = format!("header_{}", q.key);
    let group_id = format!("group_{}", q.key);
    components.push(A2uiComponent::new(
        header_id.clone(),
        A2uiComponentKind::Text {
            text: BoundString::literal(header.clone()),
            usage_hint: TextUsageHint::H3,
            tone: Some(SemanticTone::Accent),
        },
    ));
    components.push(A2uiComponent::new(
        group_id.clone(),
        A2uiComponentKind::Column {
            children: vec![header_id, field_id],
            gap: Some(0),
        },
    ));
    group_id
}

fn modal_title(questions: &[PreparedQuestion]) -> String {
    if questions.len() == 1
        && let Some(header) = &questions[0].redacted_header
    {
        return header.clone();
    }
    "Clarification".to_string()
}

fn build_question_component(field_id: &str, q: &PreparedQuestion) -> A2uiComponent {
    if q.choices.is_empty() {
        return A2uiComponent::new(
            field_id,
            A2uiComponentKind::TextField {
                label: Some(BoundString::literal(q.redacted_question.clone())),
                placeholder: Some("Type your answer...".to_string()),
                value_path: format!("/answers/{}", q.key),
                multiline: false,
                masked: false,
            },
        );
    }

    let options = q
        .choices
        .iter()
        .enumerate()
        .map(|(idx, choice)| ChoiceOption {
            value: idx.to_string(),
            label: BoundString::literal(choice.display_label.clone()),
            description: choice.description.clone().map(BoundString::literal),
        })
        .collect();

    A2uiComponent::new(
        field_id,
        A2uiComponentKind::ChoicePicker {
            label: Some(BoundString::literal(q.redacted_question.clone())),
            options,
            selection_path: format!("/answers/{}", q.key),
            multi_select: q.multi_select,
            allow_other: q.allow_other,
            other_path: q.allow_other.then(|| format!("/answers/{}_other", q.key)),
        },
    )
}

fn initial_answer_value(q: &PreparedQuestion) -> Value {
    if q.choices.is_empty() {
        return Value::String(String::new());
    }
    if q.multi_select {
        return Value::Array(Vec::new());
    }
    match q.recommended_index {
        Some(idx) => Value::String(idx.to_string()),
        None => Value::Null,
    }
}

fn append_action_buttons(components: &mut Vec<A2uiComponent>, root_children: &mut Vec<String>) {
    components.push(A2uiComponent::new(
        "cancel_btn",
        A2uiComponentKind::Button {
            label: BoundString::literal("Cancel"),
            action: ActionBinding {
                name: "cancel".to_string(),
                context: BTreeMap::new(),
            },
            variant: ButtonVariant::Secondary,
        },
    ));
    components.push(A2uiComponent::new(
        "submit_btn",
        A2uiComponentKind::Button {
            label: BoundString::literal("Submit"),
            action: ActionBinding {
                name: "submit".to_string(),
                context: BTreeMap::new(),
            },
            variant: ButtonVariant::Primary,
        },
    ));
    components.push(A2uiComponent::new(
        "actions",
        A2uiComponentKind::Row {
            children: vec!["cancel_btn".to_string(), "submit_btn".to_string()],
            gap: Some(1),
            justify: Some(RowJustify::End),
        },
    ));
    root_children.push("actions".to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_modal_request_groups_headers_with_zero_gap() {
        let questions = vec![
            PreparedQuestion {
                id: "auth".to_string(),
                key: "q1".to_string(),
                raw_question: "Pick auth method".to_string(),
                redacted_question: "Pick auth method".to_string(),
                redacted_header: Some("Auth".to_string()),
                raw_labels: Vec::new(),
                choices: Vec::new(),
                multi_select: false,
                allow_other: false,
                recommended_index: None,
            },
            PreparedQuestion {
                id: "region".to_string(),
                key: "q2".to_string(),
                raw_question: "Enter region".to_string(),
                redacted_question: "Enter region".to_string(),
                redacted_header: None,
                raw_labels: Vec::new(),
                choices: Vec::new(),
                multi_select: false,
                allow_other: false,
                recommended_index: None,
            },
        ];
        let req = build_modal_request(&questions);
        let components = req
            .messages
            .iter()
            .find_map(|msg| match msg {
                A2uiMessage::UpdateComponents(uc) => Some(&uc.components),
                _ => None,
            })
            .expect("UpdateComponents present");

        let root = components.iter().find(|c| c.id == "root").expect("root present");
        let A2uiComponentKind::Column {
            children: root_children,
            gap: root_gap,
        } = &root.kind
        else {
            panic!("expected root Column");
        };
        assert_eq!(*root_gap, Some(1));
        assert_eq!(root_children, &vec!["group_q1", "q_q2", "actions"]);

        let group = components
            .iter()
            .find(|c| c.id == "group_q1")
            .expect("group_q1 present");
        let A2uiComponentKind::Column {
            children: group_children,
            gap: group_gap,
        } = &group.kind
        else {
            panic!("expected group_q1 Column");
        };
        assert_eq!(*group_gap, Some(0));
        assert_eq!(group_children, &vec!["header_q1", "q_q1"]);
    }
}
