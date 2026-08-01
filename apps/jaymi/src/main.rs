//! Jaymi desktop application entry point.
//!
//! Milestone 1 establishes the deterministic boot sequence and a temporary
//! diagnostics window. Conversation UI and AI behavior remain out of scope.

use jaymi::{ui, Application};
use jaymi_core::{JaymiError, JaymiResult};

fn main() -> JaymiResult<()> {
    let headless = std::env::args().any(|arg| arg == "--headless");

    let mut app = Application::boot().map_err(|error| {
        JaymiError::new(format!("Jaymi failed to start: {}", error.message()))
    })?;

    if !app.state().is_ready() {
        return Err(JaymiError::new("Jaymi boot completed without Ready state"));
    }

    let snapshot = app.diagnostics()?;

    if headless {
        println!("Jaymi");
        println!("Status: {}", snapshot.app_state.label());
        println!("Planner: {}", snapshot.planner_label());
        println!("Providers: {}", snapshot.provider_count);
        println!("Tools: {}", snapshot.tool_count);
        println!("Capabilities: {}", snapshot.capability_count);
        println!("Database: {}", snapshot.database_label());
    } else {
        ui::run_diagnostics(snapshot).map_err(|error| {
            JaymiError::new(format!("desktop UI failed: {error}"))
        })?;
    }

    app.shutdown()?;
    Ok(())
}
