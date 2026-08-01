//! Jaymi desktop application entry point.
//!
//! Default launch opens the conversation-first shell.
//! CLI `read` / `list` / `--headless` continue to exercise Planner pipelines.

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
                ui::run_conversation(app)
                    .map_err(|error| JaymiError::new(format!("desktop UI failed: {error}")))?;
                return Ok(());
            }
        }
        Command::List { path } => {
            let listing = app.list_directory(&path)?;
            if headless {
                print_listing(&app, Some(listing))?;
            } else {
                ui::run_conversation(app)
                    .map_err(|error| JaymiError::new(format!("desktop UI failed: {error}")))?;
                return Ok(());
            }
        }
        Command::Conversation => {
            if headless {
                let snapshot = app.diagnostics()?;
                println!("Jaymi");
                println!("Status: {}", snapshot.app_state.label());
                println!("Planner: {}", snapshot.planner_label());
                println!("Providers: {}", snapshot.provider_count);
                println!("Tools: {}", snapshot.tool_count);
                println!("Capabilities: {}", snapshot.capability_count);
                println!("Database: {}", snapshot.database_label());
                println!();
                println!("Conversation shell ready.");
            } else {
                ui::run_conversation(app)
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
    Conversation,
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

    Ok(Command::Conversation)
}

fn print_listing(app: &Application, listing: Option<PlannerResponse>) -> JaymiResult<()> {
    let snapshot = app.diagnostics_from_response(listing)?;
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
    Ok(())
}

fn print_read_response(response: &PlannerResponse) {
    let content = match &response.content {
        Some(content) => content,
        None => {
            println!("{}", response.summary);
            return;
        }
    };

    println!("Source: {}", content.source);
    println!("Content type: {}", content.content_type);
    println!("MIME type: {}", content.mime_type);
    println!("Parser: {}", content.parser_id);
    if let Some(title) = &content.title {
        println!("Title: {title}");
    }
    if let Some(path) = &content.path {
        println!("Path: {}", path.display());
    }
    println!("Character count: {}", content.character_count());
    if let Some(created) = content.created {
        println!("Created (unix): {created}");
    }
    if let Some(modified) = content.modified {
        println!("Modified (unix): {modified}");
    }
    println!("Parsed at (unix): {}", content.parsed_at);
    println!("Metadata:");
    if content.metadata.is_empty() {
        println!("  (none)");
    } else {
        for (key, value) in content.metadata.iter() {
            println!("  {key}: {value}");
        }
    }
    println!();
    println!("Parsed text (preview):");
    println!("{}", content.preview(500));
}
