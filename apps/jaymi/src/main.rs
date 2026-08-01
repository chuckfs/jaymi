//! Jaymi desktop application entry point.
//!
//! Layer 0 — Foundation: the desktop shell wires configuration, logging,
//! database, and the Planner kernel. Conversation UI and full orchestration
//! behavior will be implemented in later milestones.

use jaymi_config::Config;
use jaymi_core::JaymiResult;
use jaymi_database::Database;
use jaymi_logging::Logger;
use jaymi_planner::Planner;

fn main() -> JaymiResult<()> {
    run()
}

/// Boot the architectural skeleton.
fn run() -> JaymiResult<()> {
    let _logger = Logger::init()?;
    let _config = Config::load()?;
    let _database = Database::open()?;
    let _planner = Planner::default();

    // Desktop UI and Planner-driven execution are intentionally deferred.
    // Layer 0 exit criteria: launch and execute a simple tool through the
    // planner — not yet implemented beyond the skeleton.
    println!("Jaymi architectural skeleton initialized.");
    Ok(())
}
