use clap::Parser;
use std::path::PathBuf;
use std::process::ExitCode;

/// A distraction-free terminal EPUB reader.
#[derive(Parser, Debug)]
#[command(name = "cleader", version, about, long_about = None)]
struct Cli {
    /// Path to an EPUB file to open.
    path: PathBuf,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("cleader: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> anyhow::Result<()> {
    // Terminal setup and event loop come in the next task.
    // For now: just open the book to verify CLI plumbing.
    let book = cleader::epub::Book::open(&cli.path)?;
    println!(
        "would open: {} by {} ({} chapters)",
        book.title,
        book.author,
        book.chapters.len()
    );
    Ok(())
}
