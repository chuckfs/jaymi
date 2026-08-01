//! Jaymi desktop application entry point.
//!
//! Supports list-directory and universal file-read pipelines through the Planner.

use std::env;
use std::path::PathBuf;

use jaymi::{ui, Application};
use jaymi_core::{JaymiError, JaymiResult};
use jaymi_planner::PlannerResponse;

fn main() -> JaymiResult<()> {
    let args: Vec<String> = env::args().collect();
    let headless = args.iter().any(|arg| arg == "--headless");
    let command = parse_command(&args)?;

    let mut app = Application::boot().map_err(|error| {
        JaymiError::new(format!("Jaymi failed to start: {}", error.message()))
    })?;

    if !app.state().is_ready() {
        return Err(JaymiError::new("Jaymi boot completed without Ready state"));
    }

    match command {
        Command::Read { path } => {
            let response = app.read_file(&path)?;
            print_read_response(&response);
            if !headless {
                let snapshot = app.diagnostics_from_response(Some(response))?;
                ui::run_diagnostics(
                    app,
                    env::current_dir()
                        .unwrap_or_else(|_| PathBuf::from("."))
                        .display()
                        .to_string(),
                    path.display().to_string(),
                    snapshot,
                )
                .map_err(|error| JaymiError::new(format!("desktop UI failed: {error}")))?;
                return Ok(());
            }
        }
        Command::List { path } | Command::Default { path } => {
            let listing = app.list_directory(&path).ok();
            let read_default = env::current_dir()
                .ok()
                .map(|cwd| cwd.join("README.md"))
                .filter(|candidate| candidate.is_file())
                .map(|candidate| candidate.display().to_string())
                .unwrap_or_default();
            let snapshot = app.diagnostics_from_response(listing)?;

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
                ui::run_diagnostics(
                    app,
                    path.display().to_string(),
                    read_default,
                    snapshot,
                )
                .map_err(|error| JaymiError::new(format!("desktop UI failed: {error}")))?;
                return Ok(());
            }
        }
    }

    app.shutdown()?;
    Ok(())
}

#[derive(Debug)]
enum Command {
    Read { path: PathBuf },
    List { path: PathBuf },
    Default { path: PathBuf },
}

fn parse_command(args: &[String]) -> JaymiResult<Command> {
    if let Some(index) = args.iter().position(|arg| arg == "read") {
        let path = args.get(index + 1).ok_or_else(|| {
            JaymiError::new("`jaymi read` requires a file path argument".to_string())
        })?;
        return Ok(Command::Read {
            path: PathBuf::from(path),
        });
    }

    if let Some(index) = args.iter().position(|arg| arg == "--list" || arg == "list") {
        let path = args.get(index + 1).ok_or_else(|| {
            JaymiError::new("list requires a directory path argument".to_string())
        })?;
        return Ok(Command::List {
            path: PathBuf::from(path),
        });
    }

    Ok(Command::Default {
        path: env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    })
}

fn print_read_response(response: &PlannerResponse) {
    let document = match &response.document {
        Some(document) => document,
        None => {
            println!("{}", response.content);
            return;
        }
    };

    println!("File type: {}", document.file_type);
    println!("Parser: {}", document.parser_id);
    if let Some(title) = &document.title {
        println!("Title: {title}");
    }
    println!("Path: {}", document.path.display());
    println!("Character count: {}", document.character_count());
    println!("Parsed at (unix): {}", document.parsed_at);
    println!("Metadata:");
    if document.metadata.is_empty() {
        println!("  (none)");
    } else {
        for (key, value) in document.metadata.iter() {
            println!("  {key}: {value}");
        }
    }
    println!();
    println!("Parsed text (preview):");
    println!("{}", document.preview(500));
}
