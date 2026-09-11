//! `aurum` — the Aurum command-line entry point.
//!
//! A thin dispatcher. Each subcommand's behaviour lives in the crate that owns
//! it, so the CLI stays an adapter rather than a second implementation. That
//! matters here because the CLI is also the automation surface: if it had its
//! own logic, tests and agents would be exercising something the Studio shell
//! does not use.

mod commands;

use std::process::ExitCode;

const USAGE: &str = "\
aurum — the Aurum engine command-line interface

USAGE:
    aurum <COMMAND> [OPTIONS]

COMMANDS:
    doctor [project]      Check a project and report healthy, warning, or
                          blocked state with evidence.
    dev [project]         Build, launch the editor, and rebuild when the Rust
                          side moves. Godot reloads its own content.
    build [project]       Build the GDExtension and install it. A failed build
                          leaves the installed library untouched.
    editor [project]      Launch Godot on the project, supervised.
    run [project]         Launch the project's game, supervised.
    new <path>            Make a project from a template. Refuses to write
                          into a directory that already has anything in it.
    stop [project]        Stop the processes this project's session launched.
    restart [project]     Restart the editor, for a native structural change
                          that cannot be migrated live. Asks politely; refuses
                          to start a second editor over one that would not
                          close unless --force is given.
    studio [project]      Start the local Studio shell and open it in a
                          browser. Loopback only; every request needs the
                          session token. See `aurum studio --help`.
    import <path>         Register a project (read-only against the project).
    projects              List registered projects.
    forget <name|path>    Remove a project from the registry.
    mcp                   Run the headless MCP server over stdio, so an AI
                          agent can drive the engine. See `aurum mcp --help`.

OPTIONS:
    -h, --help      Print this help.
    -V, --version   Print the version.

EXAMPLES:
    aurum doctor
    aurum doctor A:/path/to/project --json
    aurum import A:/path/to/project
    aurum mcp --root . --read-only
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args.first().map(String::as_str) {
        Some("mcp") => ExitCode::from(aurum_mcp::cli::run(&args[1..]) as u8),
        Some("doctor") => commands::doctor(&args[1..]),
        Some("build") => commands::build(&args[1..]),
        Some("dev") => commands::dev(&args[1..]),
        Some("editor") => commands::editor(&args[1..]),
        Some("run") => commands::run(&args[1..]),
        Some("stop") => commands::stop(&args[1..]),
        Some("restart") => commands::restart(&args[1..]),
        Some("new") => commands::new(&args[1..]),
        Some("studio") => commands::studio(&args[1..]),
        Some("import") => commands::import(&args[1..]),
        Some("projects") => commands::projects(&args[1..]),
        Some("forget") => commands::forget(&args[1..]),
        Some("-h") | Some("--help") | None => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some("-V") | Some("--version") => {
            println!("aurum {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("aurum: unknown command '{other}'\n\n{USAGE}");
            ExitCode::from(64)
        }
    }
}
