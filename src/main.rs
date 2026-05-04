use clap::Parser;
use cleader::app::App;
use cleader::epub::Book;
use cleader::input::translate;
use cleader::persistence::Persistence;
use cleader::reader::{render, RenderInput, StatusInput};
use crossterm::event;
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io;
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
    install_panic_hook();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            // Make sure terminal is restored before printing the error.
            let _ = restore_terminal();
            eprintln!("cleader: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn install_panic_hook() {
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = restore_terminal();
        original(info);
    }));
}

fn run(cli: Cli) -> anyhow::Result<()> {
    let book = Book::open(&cli.path)?;
    let persistence = Persistence::open()?;

    let mut terminal = setup_terminal()?;
    let viewport = terminal.size().map(|s| (s.width, s.height))?;
    let mut app = App::new(book, persistence, viewport);

    let result = event_loop(&mut terminal, &mut app);
    restore_terminal()?;
    result
}

fn setup_terminal() -> io::Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    Terminal::new(CrosstermBackend::new(stdout))
}

fn restore_terminal() -> io::Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    Ok(())
}

fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> anyhow::Result<()> {
    while !app.should_quit() {
        terminal.draw(|frame| {
            let area = frame.area();
            let title = app.book().title.clone();
            render(
                frame,
                area,
                RenderInput {
                    wrapped: app.wrapped(),
                    line_offset: app.line_offset(),
                    status: StatusInput {
                        title: &title,
                        chapter_display: app.main_chapter_position(),
                        page: app.page(),
                        total_pages: app.total_pages(),
                        width: area.width,
                    },
                },
            );
        })?;
        let evt = event::read()?;
        if let Some(action) = translate(evt) {
            app.handle(action);
        }
    }
    Ok(())
}
