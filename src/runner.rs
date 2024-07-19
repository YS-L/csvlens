use crate::app::App;
use crate::delimiter::Delimiter;
use crate::io::SeekableFile;

use anyhow::Result;
use clap::{command, Parser};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::ffi::OsString;
use std::panic;
use std::thread::panicking;

#[derive(Parser, Debug)]
#[command(version)]
struct Args {
    /// CSV filename
    filename: Option<String>,

    /// Delimiter character (comma by default) or "auto" to auto-detect the delimiter
    #[clap(short, long, value_name = "char")]
    delimiter: Option<String>,

    /// Use tab separation. Shortcut for -d '\t'.
    #[clap(short = 't', long)]
    tab_separated: bool,

    /// Do not interpret the first row as headers.
    #[clap(long)]
    no_headers: bool,

    /// Use this regex to select columns to display by default
    #[arg(long, value_name = "regex")]
    columns: Option<String>,

    /// Use this regex to filter rows to display by default
    #[arg(long, value_name = "regex")]
    filter: Option<String>,

    /// Use this regex to find and highlight matches by default
    #[arg(long, value_name = "regex")]
    find: Option<String>,

    /// Searches ignore case. Ignored if any uppercase letters are present in the search string
    #[clap(short, long)]
    ignore_case: bool,

    /// Print the value of this column to stdout for the selected row
    #[arg(long, value_name = "column_name")]
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
            execute!(std::io::stderr(), LeaveAlternateScreen).unwrap();
            original_panic_hook(info);
        }));

        AppRunner { app }
    }

    fn run(&mut self) -> Result<Option<String>> {
        enable_raw_mode()?;
        let mut output = std::io::stderr();
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
            execute!(std::io::stderr(), LeaveAlternateScreen).unwrap();
        }
    }
}

pub fn run_csvlens<I, T>(itr: I) -> Result<Option<String>>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let mut args_items = vec![OsString::from("csvlens")];
    for item in itr {
        args_items.push(item.into());
    }
    let args = Args::parse_from(args_items);

    let show_stats = args.debug;
    let delimiter = Delimiter::from_arg(&args.delimiter, args.tab_separated)?;

    let file = SeekableFile::new(&args.filename)?;
    let filename = file.filename();

    let app = App::new(
        filename,
        delimiter,
        args.filename,
        show_stats,
        args.echo_column,
        args.ignore_case,
        args.no_headers,
        args.columns,
        args.filter,
        args.find,
    )?;

    let mut app_runner = AppRunner::new(app);
    app_runner.run()
}
