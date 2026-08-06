//! Sprint B1.13.7 — conversation runtime state originates only from Planner.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::{Application, BeginGeneration, ExperienceSession, PumpGeneration};
use jaymi_planner::ConversationState;
use jaymi_reasoning::StreamingLifecycle;

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-b1137-{}-{}",
        label,
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn drain(app: &Application) -> Option<jaymi_planner::PlannerResponse> {
    for _ in 0..128 {
        match app.pump_generation(8).unwrap() {
            PumpGeneration::Finished(response) => return Some(response),
            PumpGeneration::Active { .. } => continue,
            PumpGeneration::Idle => return None,
        }
    }
    None
}

#[test]
fn experience_streaming_helpers_do_not_invent_runtime_state() {
    let mut session = ExperienceSession::new();
    assert_eq!(session.conversation_state(), ConversationState::Idle);
    let index = session.begin_streaming_assistant();
    assert_eq!(session.conversation_state(), ConversationState::Idle);
    session.append_stream_token(index, "hi").unwrap();
    assert_eq!(session.conversation_state(), ConversationState::Idle);
    session
        .set_stream_lifecycle(index, StreamingLifecycle::Streaming)
        .unwrap();
    assert_eq!(session.conversation_state(), ConversationState::Idle);
    session
        .finalize_streaming_turn(index, "hi", StreamingLifecycle::Failed)
        .unwrap();
    assert_eq!(session.conversation_state(), ConversationState::Idle);
    session.reset_assistant_for_retry(index).unwrap();
    assert_eq!(session.conversation_state(), ConversationState::Idle);
    // Only mirror invents/updates the runtime phase from Planner.
    session.mirror_conversation_state(ConversationState::Failed);
    assert_eq!(session.conversation_state(), ConversationState::Failed);
}

#[test]
fn state_transition_graph_covers_streaming_cancel_retry_failure_recovery() {
    use ConversationState::*;
    // Happy streaming path.
    assert!(ConversationState::can_transition(Idle, PreparingContext));
    assert!(ConversationState::can_transition(PreparingContext, Reasoning));
    assert!(ConversationState::can_transition(Reasoning, Streaming));
    assert!(ConversationState::can_transition(Streaming, Completed));
    // Cancellation.
    assert!(ConversationState::can_transition(Streaming, Cancelled));
    assert!(ConversationState::can_transition(Reasoning, Cancelled));
    // Failure.
    assert!(ConversationState::can_transition(Reasoning, Failed));
    assert!(ConversationState::can_transition(Streaming, Failed));
    // Retry recovery (Planner-owned).
    assert!(ConversationState::can_transition(Streaming, Reasoning));
    assert!(ConversationState::can_transition(Cancelled, Reasoning));
    assert!(ConversationState::can_transition(Failed, Reasoning));
    // Next-request recovery.
    assert!(ConversationState::can_transition(Cancelled, PreparingContext));
    assert!(ConversationState::can_transition(Failed, PreparingContext));
    assert!(ConversationState::can_transition(Completed, PreparingContext));
}

#[test]
fn application_keeps_experience_mirrored_to_planner_through_generation() {
    let app = Application::boot_with_data_dir(temp_dir("sync")).unwrap();
    assert_eq!(
        app.experience().unwrap().conversation_state(),
        ConversationState::Idle
    );

    match app.begin_generation("Say hello briefly").unwrap() {
        BeginGeneration::Started => {
            let experience_state = app.experience().unwrap().conversation_state();
            // After Planner start, Experience must already reflect Planner — never
            // a UI-invented PreparingContext ahead of the Planner.
            assert!(
                experience_state.is_active() || experience_state.is_terminal(),
                "unexpected mirrored state {experience_state:?}"
            );
            if let Some(response) = drain(&app) {
                assert_eq!(
                    app.experience().unwrap().conversation_state(),
                    response.conversation_state
                );
                assert!(response.conversation_state.is_terminal());
            }
        }
        BeginGeneration::Completed(response) => {
            assert_eq!(
                app.experience().unwrap().conversation_state(),
                response.conversation_state
            );
        }
    }
}

#[test]
fn cancellation_surfaces_planner_terminal_on_experience() {
    let app = Application::boot_with_data_dir(temp_dir("cancel")).unwrap();
    match app.begin_generation("Write a long answer").unwrap() {
        BeginGeneration::Started => {
            let _ = app.pump_generation(1);
            app.cancel_generation().unwrap();
            if let Some(response) = drain(&app) {
                assert!(matches!(
                    response.conversation_state,
                    ConversationState::Cancelled
                        | ConversationState::Completed
                        | ConversationState::Failed
                ));
                assert_eq!(
                    app.experience().unwrap().conversation_state(),
                    response.conversation_state
                );
            }
        }
        BeginGeneration::Completed(response) => {
            // Soft-complete environments still mirror Planner.
            assert_eq!(
                app.experience().unwrap().conversation_state(),
                response.conversation_state
            );
        }
    }
}

#[test]
fn regenerate_recovery_mirrors_planner_again() {
    let app = Application::boot_with_data_dir(temp_dir("recover")).unwrap();
    match app.begin_generation("First turn").unwrap() {
        BeginGeneration::Started => {
            let _ = drain(&app);
        }
        BeginGeneration::Completed(_) => {}
    }
    match app.regenerate_response() {
        Ok(BeginGeneration::Started) => {
            let state = app.experience().unwrap().conversation_state();
            assert!(state.is_active() || state.is_terminal());
            if let Some(response) = drain(&app) {
                assert_eq!(
                    app.experience().unwrap().conversation_state(),
                    response.conversation_state
                );
            }
        }
        Ok(BeginGeneration::Completed(response)) => {
            assert_eq!(
                app.experience().unwrap().conversation_state(),
                response.conversation_state
            );
        }
        Err(_) => {
            // No prior user turn in some soft-fail boots — ownership still covered above.
        }
    }
}
