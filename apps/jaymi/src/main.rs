//! Jaymi desktop application entry point.
//!
//! Milestone 2 proves the architecture with a list-directory request flowing
//! Planner → Capability → Tool → Provider → Filesystem.

use std::env;
use std::path::PathBuf;

use jaymi::{ui, Application};
use jaymi_core::{JaymiError, JaymiResult};

fn main() -> JaymiResult<()> {
    let args: Vec<String> = env::args().collect();
    let headless = args.iter().any(|arg| arg == "--headless");
    let list_path = parse_list_path(&args)?;

    let mut app = Application::boot().map_err(|error| {
        JaymiError::new(format!("Jaymi failed to start: {}", error.message()))
    })?;

    if !app.state().is_ready() {
        return Err(JaymiError::new("Jaymi boot completed without Ready state"));
    }

    let default_path = list_path.unwrap_or_else(|| {
        env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    });

    let listing = app.list_directory(&default_path).ok();
    let snapshot = app.diagnostics_with_listing(listing)?;

    if headless {
        println!("Jaymi");
        println!("Status: {}", snapshot.app_state.label());
        println!("Planner: {}", snapshot.planner_label());
        println!("Providers: {}", snapshot.provider_count);
        println!("Tools: {}", snapshot.tool_count);
        println!("Capabilities: {}", snapshot.capability_count);
        println!("Database: {}", snapshot.database_label());
        if let Some(summary) = &snapshot.listing_summary {
            println!();
            println!("{summary}");
        }
        for entry in &snapshot.entries {
            println!(
                "{}\t{}\t{}\t{}\t{}",
                entry.name,
                entry.entry_type,
                entry.path.display(),
                entry.size,
                entry
                    .modified
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "-".to_string())
            );
        }
    } else {
        ui::run_diagnostics(app, default_path.display().to_string(), snapshot).map_err(
            |error| JaymiError::new(format!("desktop UI failed: {error}")),
        )?;
        return Ok(());
    }

    app.shutdown()?;
    Ok(())
}

fn parse_list_path(args: &[String]) -> JaymiResult<Option<PathBuf>> {
    if let Some(index) = args.iter().position(|arg| arg == "--list") {
        let path = args.get(index + 1).ok_or_else(|| {
            JaymiError::new("--list requires a directory path argument".to_string())
        })?;
        Ok(Some(PathBuf::from(path)))
    } else {
        Ok(None)
    }
}
