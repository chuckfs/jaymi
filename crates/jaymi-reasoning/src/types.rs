//! Conversation turns for reasoning history.

use serde::{Deserialize, Serialize};

/// Role of a turn in a reasoning conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationRole {
    /// End-user utterance.
    User,
    /// Assistant / model utterance.
    Assistant,
    /// System guidance (instructions, style, constraints).
    System,
    /// Tool / capability observation returned into the conversation.
    Tool,
}

impl ConversationRole {
    /// Stable label.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::System => "system",
            Self::Tool => "tool",
        }
    }
}

/// One conversational turn supplied to or returned from reasoning.
///
/// Distinct from Experience UI turns — this is the Reasoning contract shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationTurn {
    /// Speaker role.
    pub role: ConversationRole,
    /// Turn text content.
    pub content: String,
    /// Optional tool name when [`ConversationRole::Tool`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    /// Optional stable turn id for correlation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
}

impl ConversationTurn {
    /// User turn.
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: ConversationRole::User,
            content: content.into(),
            tool_name: None,
            turn_id: None,
        }
    }

    /// Assistant turn.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: ConversationRole::Assistant,
            content: content.into(),
            tool_name: None,
            turn_id: None,
        }
    }

    /// System turn.
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: ConversationRole::System,
            content: content.into(),
            tool_name: None,
            turn_id: None,
        }
    }

    /// Tool observation turn.
    pub fn tool(tool_name: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: ConversationRole::Tool,
            content: content.into(),
            tool_name: Some(tool_name.into()),
            turn_id: None,
        }
    }

    /// Attach a turn id.
    pub fn with_id(mut self, turn_id: impl Into<String>) -> Self {
        self.turn_id = Some(turn_id.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_labels() {
        assert_eq!(ConversationRole::User.as_str(), "user");
        assert_eq!(ConversationRole::Tool.as_str(), "tool");
    }

    #[test]
    fn constructors_set_role() {
        assert_eq!(ConversationTurn::user("hi").role, ConversationRole::User);
        assert_eq!(
            ConversationTurn::tool("search", "hits").tool_name.as_deref(),
            Some("search")
        );
    }
}
