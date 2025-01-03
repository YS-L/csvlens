use crate::app::App;
use crate::delimiter::Delimiter;
use crate::errors::CsvlensResult;
use crate::io::SeekableFile;

#[cfg(feature = "cli")]
use clap::{command, Parser};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::ffi::OsString;
use std::io::LineWriter;
use std::panic;
use std::thread::panicking;

#[cfg(feature = "cli")]
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

// Struct for library usage without clap directives
#[cfg(not(feature = "cli"))]
#[derive(Debug)]
pub struct CsvlensOptions {
    pub filename: Option<String>,
    pub delimiter: Option<String>,
    pub tab_separated: bool,
    pub no_headers: bool,
    pub columns: Option<String>,
    pub filter: Option<String>,
    pub find: Option<String>,
    pub ignore_case: bool,
    pub echo_column: Option<String>,
    pub debug: bool,
}

#[cfg(not(feature = "cli"))]
impl Default for CsvlensOptions {
    fn default() -> Self {
        Self {
            filename: None,
            delimiter: None,
            tab_separated: false,
            no_headers: false,
            columns: None,
            filter: None,
            find: None,
            ignore_case: false,
            echo_column: None,
            debug: false,
        }
    }
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

    fn run(&mut self) -> CsvlensResult<Option<String>> {
        enable_raw_mode()?;
        let mut output = std::io::stderr();
        execute!(output, EnterAlternateScreen)?;

        let backend = CrosstermBackend::new(LineWriter::new(output));
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

/// Run csvlens with a list of arguments. The accepted arguments are the same as the command line
/// arguments for the csvlens binary.
///
/// On success, the result contains an optional string that is the value of the selected cell if
/// any. If csvlens exits without selecting a cell, the result is None.
///
/// Example:
///
/// ```
/// use csvlens::run_csvlens;
///
/// match run_csvlens(&["/path/to/your.csv", "--delimiter", "\t"]) {
///     Ok(Some(selected_cell)) => println!("Selected: {}", selected_cell),
///     Ok(None) => {},
///     Err(e) => eprintln!("Error: {:?}", e),
/// }
/// ```
#[cfg(feature = "cli")]
pub fn run_csvlens<I, T>(args: I) -> CsvlensResult<Option<String>>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    {
        let mut args_items = vec![OsString::from("csvlens")];
        for item in args {
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
}

#[cfg(not(feature = "cli"))]
pub fn run_csvlens_with_options(options: CsvlensOptions) -> CsvlensResult<Option<String>> {
    let show_stats = options.debug;
    let delimiter = Delimiter::from_arg(&options.delimiter, options.tab_separated)?;

    let file = SeekableFile::new(&options.filename)?;
    let filename = file.filename();

    let app = App::new(
        filename,
        delimiter,
        options.filename,
        show_stats,
        options.echo_column,
        options.ignore_case,
        options.no_headers,
        options.columns,
        options.filter,
        options.find,
    )?;

    let mut app_runner = AppRunner::new(app);
    app_runner.run()
}

#[cfg(not(feature = "cli"))]
pub fn run_csvlens<I, T>(args: I) -> CsvlensResult<Option<String>>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let args_vec: Vec<OsString> = args.into_iter().map(|x| x.into()).collect();

    let options = CsvlensOptions {
        filename: args_vec.get(0).map(|s| s.to_string_lossy().into_owned()),
        ..Default::default()
    };

    run_csvlens_with_options(options)
}
