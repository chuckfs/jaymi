//! Conversation state for Jaymi's conversation-first interface.
//!
//! Messages are UI-facing representations. The Planner remains the authority
//! for fulfilling requests; this module only stores what the conversation shows.

use std::time::{SystemTime, UNIX_EPOCH};

/// Who authored a conversation message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageRole {
    /// Message shown as coming from Jaymi.
    Assistant,
    /// Message authored by the user.
    User,
    /// System / ambient notice (welcome, settings feedback).
    System,
}

impl MessageRole {
    /// Stable identity for tests and diagnostics.
    pub fn id(&self) -> &'static str {
        match self {
            Self::Assistant => "assistant",
            Self::User => "user",
            Self::System => "system",
        }
    }
}

/// One message in the conversation transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    /// Stable message identity within the session.
    pub id: String,
    /// Message author role.
    pub role: MessageRole,
    /// Visible message text.
    pub text: String,
}

impl ChatMessage {
    /// Create a message with an auto-generated id.
    pub fn new(role: MessageRole, text: impl Into<String>) -> Self {
        Self {
            id: next_message_id(role),
            role,
            text: text.into(),
        }
    }

    /// Jaymi welcome greeting shown when a conversation begins.
    pub fn welcome() -> Self {
        Self::new(
            MessageRole::Assistant,
            "Hi, I'm Jaymi.\n\nWhat would you like to work on today?",
        )
    }
}

/// In-memory conversation transcript for the desktop shell.
#[derive(Debug, Clone)]
pub struct Conversation {
    messages: Vec<ChatMessage>,
}

impl Conversation {
    /// Start a conversation with the welcome message.
    pub fn with_welcome() -> Self {
        Self {
            messages: vec![ChatMessage::welcome()],
        }
    }

    /// Borrow the transcript chronologically.
    pub fn messages(&self) -> &[ChatMessage] {
        &self.messages
    }

    /// Append a user message.
    pub fn push_user(&mut self, text: impl Into<String>) -> &ChatMessage {
        self.messages.push(ChatMessage::new(MessageRole::User, text));
        self.messages.last().expect("just pushed")
    }

    /// Append an assistant message.
    pub fn push_assistant(&mut self, text: impl Into<String>) -> &ChatMessage {
        self.messages
            .push(ChatMessage::new(MessageRole::Assistant, text));
        self.messages.last().expect("just pushed")
    }

    /// Append a system notice.
    pub fn push_system(&mut self, text: impl Into<String>) -> &ChatMessage {
        self.messages
            .push(ChatMessage::new(MessageRole::System, text));
        self.messages.last().expect("just pushed")
    }

    /// Number of messages in the transcript.
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    /// Returns true when the transcript is empty.
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }
}

impl Default for Conversation {
    fn default() -> Self {
        Self::with_welcome()
    }
}

fn next_message_id(role: MessageRole) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("{}-{nanos}", role.id())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn welcome_conversation_starts_with_jaymi() {
        let conversation = Conversation::with_welcome();
        assert_eq!(conversation.len(), 1);
        assert_eq!(conversation.messages()[0].role, MessageRole::Assistant);
        assert!(conversation.messages()[0].text.contains("Hi, I'm Jaymi"));
    }

    #[test]
    fn appends_user_and_assistant_turns() {
        let mut conversation = Conversation::with_welcome();
        conversation.push_user("list .");
        conversation.push_assistant("I found 1 item.");
        assert_eq!(conversation.len(), 3);
        assert_eq!(conversation.messages()[1].role, MessageRole::User);
        assert_eq!(conversation.messages()[2].role, MessageRole::Assistant);
    }
}
