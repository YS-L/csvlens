mod app;
mod csv;
mod delimiter;
mod find;
mod help;
mod input;
mod ui;
#[allow(dead_code)]
mod util;
mod view;
mod wrap;
use crate::app::App;
use crate::delimiter::Delimiter;

extern crate csv as sushi_csv;

use anyhow::{Context, Result};
use clap::{command, Parser};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::panic;
use std::thread::panicking;
use tempfile::NamedTempFile;

struct SeekableFile {
    filename: Option<String>,
    inner_file: Option<NamedTempFile>,
}

impl SeekableFile {
    fn new(maybe_filename: &Option<String>) -> Result<SeekableFile> {
        let mut inner_file = NamedTempFile::new()?;
        let inner_file_res;

        if let Some(filename) = maybe_filename {
            let err = format!("Failed to open file: {filename}");
            let mut f = File::open(filename).context(err)?;
            // If not seekable, it most likely is due to process substitution using
            // pipe - write out to a temp file to make it seekable
            if f.seek(SeekFrom::Start(0)).is_err() {
                Self::chunked_copy(&mut f, &mut inner_file)?;
                inner_file_res = Some(inner_file);
            } else {
                inner_file_res = None;
            }
        } else {
            // Handle input from stdin
            let mut stdin = std::io::stdin();
            Self::chunked_copy(&mut stdin, &mut inner_file)?;
            inner_file_res = Some(inner_file);
        }

        Ok(SeekableFile {
            filename: maybe_filename.clone(),
            inner_file: inner_file_res,
        })
    }

    fn filename(&self) -> &str {
        if let Some(f) = &self.inner_file {
            f.path().to_str().unwrap()
        } else {
            // If data is from stdin, then inner_file must be there
            self.filename.as_ref().unwrap()
        }
    }

    fn chunked_copy<R: Read, W: Write>(source: &mut R, dest: &mut W) -> Result<usize> {
        let mut total_copied = 0;
        let mut buffer = vec![0; 1_000_000];
        loop {
            let n = source.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            let n_written = dest.write(&buffer[..n])?;
            total_copied += n_written;
        }
        Ok(total_copied)
    }
}

#[derive(Parser, Debug)]
#[command(version)]
struct Args {
    /// CSV filename
    filename: Option<String>,

    /// Delimiter character (comma by default) or "auto" to auto-detect the delimiter
    #[clap(short, long)]
    delimiter: Option<String>,

    /// Searches ignore case. Ignored if any uppercase letters are present in the search string
    #[clap(short, long)]
    ignore_case: bool,

    /// Print the value of this column to stdout for the selected row
    #[arg(long)]
    echo_column: Option<String>,

    /// Show stats for debugging
    #[clap(long)]
    debug: bool,
}

struct AppRunner {
    app: App,
}

impl AppRunner {
    fn new(app: App) -> AppRunner {
        let original_panic_hook = panic::take_hook();

        panic::set_hook(Box::new(move |info| {
            // Restore terminal states first so that the backtrace on panic can
            // be printed with proper line breaks
            disable_raw_mode().unwrap();
            execute!(io::stderr(), LeaveAlternateScreen).unwrap();
            original_panic_hook(info);
        }));

        AppRunner { app }
    }

    fn run(&mut self) -> Result<Option<String>> {
        enable_raw_mode()?;
        let mut output = io::stderr();
        execute!(output, EnterAlternateScreen)?;

        let backend = CrosstermBackend::new(output);
        let mut terminal = Terminal::new(backend)?;

        self.app.main_loop(&mut terminal)
    }
}

impl Drop for AppRunner {
    fn drop(&mut self) {
        // If panicked, restoring of terminal states would have been done in the
        // panic hook. Avoid doing that twice since that would clear the printed
        // backtrace.
        if !panicking() {
            disable_raw_mode().unwrap();
            execute!(io::stderr(), LeaveAlternateScreen).unwrap();
        }
    }
}

fn run_csvlens() -> Result<Option<String>> {
    let args = Args::parse();

    let show_stats = args.debug;
    let delimiter = Delimiter::from_arg(&args.delimiter)?;

    let file = SeekableFile::new(&args.filename)?;
    let filename = file.filename();

    let app = App::new(
        filename,
        delimiter,
        args.filename,
        show_stats,
        args.echo_column,
        args.ignore_case,
    )?;

    let mut app_runner = AppRunner::new(app);
    app_runner.run()
}

fn main() {
    match run_csvlens() {
        Err(e) => {
            println!("{e}");
            std::process::exit(1);
        }
        Ok(Some(selection)) => {
            println!("{selection}");
        }
        _ => {}
    }
}
