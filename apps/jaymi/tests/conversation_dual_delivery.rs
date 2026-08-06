//! Sprint B1.13.8 — streaming (pumpable) vs blocking conversational delivery integrity.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use std::sync::Arc;

use jaymi::{Application, BeginGeneration, PumpGeneration};
use jaymi_core::UserRequest;

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-b1138-{}-{}",
        label,
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn drain(app: &Arc<Application>) -> Option<jaymi_planner::PlannerResponse> {
    for _ in 0..200 {
        match app.pump_generation(8).unwrap() {
            PumpGeneration::Finished(response) => return Some(response),
            PumpGeneration::Active { .. } => {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            PumpGeneration::Idle => return None,
        }
    }
    None
}

#[test]
fn blocking_streaming_records_planner_activity() {
    let app = Arc::new(Application::boot_with_data_dir(temp_dir("blocking-activity")).unwrap());
    let response = app
        .handle_streaming_with_workspace(UserRequest::new("Say hi briefly"))
        .expect("blocking streaming handle");
    let activity = app
        .last_planner_activity()
        .expect("handle_streaming_with_workspace must record planner activity");
    assert_eq!(activity.summary, response.content);
    assert!(!activity.summary.is_empty());
}

#[test]
fn pumpable_and_blocking_both_prepare_context_session() {
    let app = Arc::new(Application::boot_with_data_dir(temp_dir("shared-prep")).unwrap());
    let context = app
        .container()
        .resolve::<std::sync::Arc<jaymi_context::ContextEngine>>()
        .expect("context");

    let _ = app
        .handle_with_workspace(UserRequest::new("Blocking hello"))
        .expect("blocking");
    let after_blocking = context.session_inputs();
    assert!(
        !after_blocking.permissions.entries.is_empty(),
        "blocking path must run prepare_context_session"
    );

    match app.begin_generation("Pumpable hello").unwrap() {
        BeginGeneration::Started => {
            let _ = drain(&app);
        }
        BeginGeneration::Completed(_) => {}
    }
    let after_pump = context.session_inputs();
    assert!(
        !after_pump.permissions.entries.is_empty(),
        "pumpable path must run prepare_context_session"
    );
}

#[test]
fn pumpable_finished_response_retains_core_diagnostics_fields() {
    let app = Arc::new(Application::boot_with_data_dir(temp_dir("pump-diag")).unwrap());
    match app.begin_generation("What is a borrow checker?").unwrap() {
        BeginGeneration::Started => {
            if let Some(response) = drain(&app) {
                assert!(response.stream_lifecycle.is_some());
                assert!(response.conversation_state.is_terminal());
                // Soft stream-start failures set reasoning_used without prompt/metrics;
                // successful reasoning turns retain at least one diagnostics field.
                if response.reasoning_used
                    && !matches!(
                        response.stream_lifecycle,
                        Some(jaymi_reasoning::StreamingLifecycle::Failed)
                    )
                {
                    assert!(
                        response.prompt_diagnostics.is_some()
                            || response.reasoning_metrics.is_some(),
                        "successful reasoning turns must retain prompt or metrics diagnostics"
                    );
                }
                assert!(app.last_planner_activity().is_some());
            }
        }
        BeginGeneration::Completed(_) => {
            panic!("interactive begin_generation must not complete synchronously");
        }
    }
}
