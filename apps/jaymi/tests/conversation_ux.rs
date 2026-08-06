//! Conversation UX polish (Sprint B1.11) — interaction helpers + generation APIs.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use std::sync::Arc;

use jaymi::{
    action_accessibility_label, caret_blink_on, display_content, loading_opacity,
    progress_accessibility_label, show_typing_indicator, smooth_streaming_text, turn_actions,
    Application, BeginGeneration, ConversationTurn, ConversationTurnActions, ExperienceSession,
    PumpGeneration,
};
use jaymi_memory::MessageRole;
use jaymi_planner::ConversationState;
use jaymi_reasoning::StreamingLifecycle;

fn temp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("jaymi-b111-{label}-{nanos}"));
    std::fs::create_dir_all(&path).expect("temp dir");
    path
}

#[test]
fn interaction_helpers_cover_actions_cursor_and_loading() {
    let mut turn = ConversationTurn::assistant("Hello Jaymi");
    turn.stream_lifecycle = Some(StreamingLifecycle::Completed);
    let actions = turn_actions(&turn);
    assert_eq!(
        actions,
        ConversationTurnActions {
            copy: true,
            retry: false,
            regenerate: true,
        }
    );

    turn.stream_lifecycle = Some(StreamingLifecycle::Streaming);
    assert_eq!(turn_actions(&turn), ConversationTurnActions::default());
    assert!(display_content(&turn, true).ends_with('\u{258C}'));
    assert_eq!(smooth_streaming_text("x", true, false), "x");
    assert!(loading_opacity(1.0) >= 0.99);
    assert!(caret_blink_on(0.2));
    assert!(!show_typing_indicator(ConversationState::Streaming, true));
}

#[test]
fn accessibility_labels_describe_progress_and_actions() {
    let progress = progress_accessibility_label(ConversationState::PreparingContext);
    assert!(progress.to_lowercase().contains("preparing") || progress.contains("Jaymi"));
    let copy = action_accessibility_label("Copy response", "Ownership in Rust");
    assert!(copy.contains("Copy response"));
    assert!(copy.contains("Ownership"));
}

#[test]
fn streaming_session_helpers_support_retry_flow() {
    let mut session = ExperienceSession::new();
    session.record_user_message("Explain");
    let index = session.begin_streaming_assistant();
    session.append_stream_token(index, "Hi").unwrap();
    assert_eq!(session.active_streaming_turn_index(), Some(index));
    session
        .finalize_streaming_turn(index, "Hi", StreamingLifecycle::Failed)
        .unwrap();
    assert!(session.conversation()[index].is_retryable());
    session.reset_assistant_for_retry(index).unwrap();
    assert!(session.conversation()[index].is_streaming());
}

#[test]
fn generation_cancel_and_pump_idle_without_active_stream() {
    let app = Arc::new(Application::boot_with_data_dir(temp_dir("cancel")).expect("boot"));
    assert!(!app.generation_active());
    assert!(matches!(
        app.pump_generation(4).unwrap(),
        PumpGeneration::Idle
    ));
    app.cancel_generation().unwrap();
}

#[test]
fn begin_generation_acks_thinking_without_blocking() {
    let app = Arc::new(Application::boot_with_data_dir(temp_dir("ack")).expect("boot"));
    let started = std::time::Instant::now();
    let outcome = app.begin_generation("What is ownership?").unwrap();
    let elapsed = started.elapsed();
    assert!(
        matches!(outcome, BeginGeneration::Started),
        "interactive send must return Started immediately"
    );
    assert!(
        app.generation_active(),
        "generation slot must be occupied while background start runs"
    );
    assert!(
        elapsed < std::time::Duration::from_millis(250),
        "begin_generation must return before prep/assemble/stream-open (took {elapsed:?})"
    );
    let planner = app
        .container()
        .resolve::<jaymi_planner::Planner>()
        .expect("planner");
    assert_eq!(
        planner.conversation_state(),
        ConversationState::Reasoning,
        "Planner must ack Thinking before background work"
    );
    let session = app.experience().unwrap();
    assert!(
        session
            .conversation()
            .iter()
            .any(|turn| matches!(turn.role, MessageRole::User)),
        "user turn must be recorded on ack"
    );
    assert!(
        session.has_streaming_turn(),
        "Thinking assistant turn must exist on ack"
    );
    assert_eq!(session.conversation_state(), ConversationState::Reasoning);

    app.cancel_generation().unwrap();
    for _ in 0..64 {
        match app.pump_generation(8).unwrap() {
            PumpGeneration::Finished(_) | PumpGeneration::Idle => break,
            PumpGeneration::Active { .. } => {}
        }
    }
}

#[test]
fn begin_generation_streaming_or_soft_completion() {
    let app = Arc::new(Application::boot_with_data_dir(temp_dir("begin")).expect("boot"));
    match app.begin_generation("What is ownership?").unwrap() {
        BeginGeneration::Started => {
            assert!(app.generation_active());
            // Soft-fail / stream may finish via pump; cancel is always safe.
            app.cancel_generation().unwrap();
            for _ in 0..64 {
                match app.pump_generation(8).unwrap() {
                    PumpGeneration::Finished(_) | PumpGeneration::Idle => break,
                    PumpGeneration::Active { .. } => {}
                }
            }
            assert!(!app.generation_active());
            let session = app.experience().unwrap();
            assert!(session.turn_count() >= 2);
        }
        BeginGeneration::Completed(_) => {
            panic!("interactive begin_generation must not complete synchronously");
        }
    }
}

#[test]
fn clipboard_retry_and_regenerate_apis() {
    let app = Arc::new(Application::boot_with_data_dir(temp_dir("retry")).expect("boot"));
    let _ = app.begin_generation("Say hello").unwrap();
    if app.generation_active() {
        app.cancel_generation().unwrap();
        for _ in 0..64 {
            match app.pump_generation(8).unwrap() {
                PumpGeneration::Finished(_) | PumpGeneration::Idle => break,
                PumpGeneration::Active { .. } => {}
            }
        }
    }
    let session = app.experience().unwrap();
    if session.turn_count() < 2 {
        return;
    }
    let index = session
        .conversation()
        .iter()
        .rposition(|turn| matches!(turn.role, jaymi_memory::MessageRole::Assistant))
        .expect("assistant");
    let text = app.assistant_turn_text(index).unwrap();
    assert_eq!(text, session.conversation()[index].content);

    let _ = app.regenerate_response();
    if app.generation_active() {
        app.cancel_generation().unwrap();
        for _ in 0..64 {
            match app.pump_generation(8).unwrap() {
                PumpGeneration::Finished(_) | PumpGeneration::Idle => break,
                PumpGeneration::Active { .. } => {}
            }
        }
    }
}
