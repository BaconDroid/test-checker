use clap::Parser;

use crate::api::{EXIT_PASS, check_hook_input};

#[must_use]
pub fn run_from_env() -> i32 {
    let args = Args::parse();
    run(&args)
}

#[must_use]
pub fn run(args: &Args) -> i32 {
    let mut input = String::new();
    if std::io::Read::read_to_string(&mut std::io::stdin(), &mut input).is_err() {
        eprintln!("[test-checker] Skipping: Failed to read stdin");
        return EXIT_PASS;
    }
    run_with_input(&input, args.prompt.as_deref().unwrap_or_default())
}

#[derive(Debug, Parser)]
#[command(
    name = "test-checker",
    version,
    about = "Check for problematic tests in source code",
    long_about = "A hook for Claude Code that detects tests weakened in the latest edit."
)]
pub struct Args {
    #[arg(long)]
    pub prompt: Option<String>,
}

#[must_use]
pub fn run_with_input(input: &str, custom_prompt: &str) -> i32 {
    let result = check_hook_input(input, custom_prompt);
    eprint!("{}", result.message);
    result.exit_code
}
