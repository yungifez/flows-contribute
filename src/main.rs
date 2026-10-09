use flows_contribute::agent::Kind;
use flows_contribute::commands::{self, RunOptions};
use std::process::ExitCode;

const USAGE: &str = "flows-contribute: label code for the Flows project with your own coding agent.

Usage:
  flows-contribute login                 Log in with GitHub.
  flows-contribute run [options]         Label batches until your usage limit or the daily limit.
      --agent codex|claude               The agent to use. Default: Codex if it is installed.
      --model <model>                    The model to use. Default: the first model Flows accepts.
      --once                             Label one batch, then stop.
  flows-contribute status                Show your contributor status.
  flows-contribute logout                Remove your login from this computer.
  flows-contribute --version";

fn parse_run(args: &[String]) -> Result<RunOptions, String> {
    let mut options = RunOptions {
        agent: None,
        model: None,
        once: false,
    };
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--agent" => {
                let value = iter
                    .next()
                    .ok_or("--agent needs a value: codex or claude")?;
                options.agent = Some(Kind::parse(value).ok_or("--agent must be codex or claude")?);
            }
            "--model" => options.model = Some(iter.next().ok_or("--model needs a value")?.clone()),
            "--once" => options.once = true,
            other => return Err(format!("Unknown option: {other}\n\n{USAGE}")),
        }
    }
    Ok(options)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("login") if args.len() == 1 => commands::login(),
        Some("logout") if args.len() == 1 => commands::logout(),
        Some("status") if args.len() == 1 => commands::status(),
        Some("run") => parse_run(&args[1..]).and_then(commands::run),
        Some("--version") | Some("-V") => {
            println!("flows-contribute {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some("--help") | Some("-h") | Some("help") | None => {
            println!("{USAGE}");
            Ok(())
        }
        _ => Err(format!("Unknown command.\n\n{USAGE}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(problem) => {
            eprintln!("\nProblem: {problem}");
            ExitCode::FAILURE
        }
    }
}
