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
    /// Path to an EPUB file, or a directory containing EPUBs. When a
    /// directory is given, cleader shows a library list view where you
    /// can pick a book to read.
    path: PathBuf,

    /// Target body text width in columns. Defaults to 80, the
    /// readability sweet spot for fiction. Useful on wide terminals
    /// where 80 leaves too much whitespace, or on smaller windows
    /// where you want the text to use more available space. Values
    /// below 20 are silently clamped (the wrap algorithm needs at
    /// least a few words per line to work well).
    #[arg(short = 'w', long, default_value_t = cleader::reader::DEFAULT_MAX_BODY_WIDTH)]
    width: u16,
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
    if cli.path.is_dir() {
        run_library_session(&cli.path, cli.width)
    } else {
        run_single_book_session(&cli.path, cli.width)
    }
}

fn run_single_book_session(
    path: &std::path::Path,
    max_body_width: u16,
) -> anyhow::Result<()> {
    let book = Book::open(path, max_body_width)?;
    let persistence = Persistence::open()?;

    let mut terminal = setup_terminal()?;
    let viewport = terminal.size().map(|s| (s.width, s.height))?;
    let mut app = App::new(book, persistence, viewport, max_body_width);

    let result = event_loop(&mut terminal, &mut app);
    restore_terminal()?;
    result
}

fn run_library_session(
    dir: &std::path::Path,
    max_body_width: u16,
) -> anyhow::Result<()> {
    let entries = cleader::library::scan_directory(dir)?;
    if entries.is_empty() {
        return Err(anyhow::anyhow!(
            "no EPUBs found in {} (scan does not recurse into subdirectories)",
            dir.display()
        ));
    }

    let mut terminal = setup_terminal()?;
    let viewport = terminal.size().map(|s| (s.width, s.height))?;
    let mut library_app = cleader::library_app::LibraryApp::new(entries, viewport);

    let outcome: anyhow::Result<()> = loop {
        // Run library until selection or quit.
        if let Err(e) = library_event_loop(&mut terminal, &mut library_app) {
            break Err(e);
        }

        // No selection -> user quit out of library mode entirely.
        let Some(book_path) = library_app.selected_path() else {
            break Ok(());
        };
        let book_path = book_path.to_path_buf();

        // Open the book and run the reader.
        let book = match Book::open(&book_path, max_body_width) {
            Ok(b) => b,
            Err(e) => break Err(e.into()),
        };
        let persistence = match Persistence::open() {
            Ok(p) => p,
            Err(e) => break Err(e.into()),
        };
        let viewport = match terminal.size() {
            Ok(s) => (s.width, s.height),
            Err(e) => break Err(e.into()),
        };
        let mut app = App::new(book, persistence, viewport, max_body_width);
        if let Err(e) = event_loop(&mut terminal, &mut app) {
            break Err(e);
        }

        // Reader exited. Update library viewport (terminal may have been
        // resized during the reader session) and clear the should_quit
        // and selected_path flags so the next iteration starts fresh.
        if let Ok(s) = terminal.size() {
            library_app.set_viewport((s.width, s.height));
        }
        library_app.reset_for_reselection();
    };

    restore_terminal()?;
    outcome
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
            let toc = if app.show_toc() {
                let entries: Vec<(String, cleader::epub::ChapterKind)> = app
                    .book()
                    .chapters
                    .iter()
                    .enumerate()
                    .map(|(idx, ch)| {
                        let label = ch
                            .title
                            .clone()
                            .unwrap_or_else(|| format!("Chapter {}", idx + 1));
                        (label, ch.kind)
                    })
                    .collect();
                Some(cleader::reader::TocOverlay {
                    entries,
                    selection: app.toc_selection(),
                    current_chapter: app.chapter_idx(),
                })
            } else {
                None
            };
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
                        warning: app.save_error(),
                        width: area.width,
                    },
                    show_help: app.show_help(),
                    max_body_width: app.max_body_width(),
                    toc,
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

fn library_event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut cleader::library_app::LibraryApp,
) -> anyhow::Result<()> {
    while !app.should_quit() {
        terminal.draw(|frame| {
            let area = frame.area();
            cleader::reader::render_library(
                frame,
                area,
                cleader::reader::LibraryRenderInput {
                    entries: app.entries(),
                    selection: app.selection(),
                    view_mode: app.view_mode(),
                    cover_cache: app.cover_cache(),
                    book_ids: app.book_ids(),
                    warning: app.save_error(),
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
