//! Barca CLI — invisible asset orchestrator.

mod cache;
mod commands;
mod db;
mod dispatch;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: barca run <python_file> [python_file ...]");
        std::process::exit(1);
    }

    match args[1].as_str() {
        "run" => commands::run_command(&args[2..]),
        "get" => commands::get_command(&args[2..]),
        "plan" => commands::plan_command(&args[2..]),
        "--help" | "-h" => {
            println!("barca — invisible asset orchestrator");
            println!();
            println!("Commands:");
            println!("  get <target> <file>     Get a fresh asset (cache-aware)");
            println!("  run <file> [file ...]   Parse, plan, and execute all assets");
            println!("  plan <file> [file ...]  Parse and emit execution plan (JSON)");
        }
        _ => commands::run_command(&args[1..]),
    }
}
