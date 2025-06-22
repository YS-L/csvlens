use crate::app::{App, WrapMode};
use crate::delimiter::Delimiter;
use crate::errors::CsvlensResult;
use crate::io::SeekableFile;

#[cfg(feature = "cli")]
use clap::{Parser, command};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
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
    ///
    /// Example: "column1|column2" matches "column1", "column2", and also column names like
    /// "column11", "column22".
    #[arg(long, value_name = "regex")]
    columns: Option<String>,

    /// Use this regex to filter rows to display by default
    ///
    /// The regex is matched against each cell in every column.
    ///
    /// Example: "value1|value2" filters rows with any cells containing "value1", "value2", or text
    /// like "my_value1" or "value234".
    #[arg(long, value_name = "regex")]
    filter: Option<String>,

    /// Use this regex to find and highlight matches by default
    ///
    /// The regex is matched against each cell in every column.
    ///
    /// Example: "value1|value2" highlights text in any cells containing "value1", "value2", or
    /// longer text like "value1_ok".
    #[arg(long, value_name = "regex")]
    find: Option<String>,

    /// Searches ignore case. Ignored if any uppercase letters are present in the search string
    #[clap(short, long)]
    ignore_case: bool,

    /// Print the value of this column to stdout for the selected row
    #[arg(long, value_name = "column_name")]
    echo_column: Option<String>,

    /// Whether to display each column in a different color
    #[arg(long, alias = "colorful", visible_alias = "colorful")]
    color_columns: bool,

    /// Show a custom prompt message in the status bar. Supports ANSI escape codes for colored or
    /// styled text.
    #[arg(long, value_name = "prompt")]
    prompt: Option<String>,

    /// Show stats for debugging
    #[clap(long)]
    debug: bool,
}

#[cfg(feature = "cli")]
impl From<Args> for CsvlensOptions {
    fn from(args: Args) -> Self {
        Self {
            filename: args.filename,
            delimiter: args.delimiter,
            tab_separated: args.tab_separated,
            no_headers: args.no_headers,
            columns: args.columns,
            filter: args.filter,
            find: args.find,
            ignore_case: args.ignore_case,
            echo_column: args.echo_column,
            debug: args.debug,
            freeze_cols_offset: None,
            color_columns: args.color_columns,
            prompt: args.prompt,
            wrap_mode: None,
        }
    }
}

// Struct for library usage without clap directives
#[derive(Debug, Default)]
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
    pub freeze_cols_offset: Option<u64>,
    pub color_columns: bool,
    pub prompt: Option<String>,
    pub wrap_mode: Option<WrapMode>,
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

/// Run csvlens with options provided in a `CsvlensOptions` struct.
///
/// On success, the result contains an optional string that is the value of the selected cell if
/// any. If csvlens exits without selecting a cell, the result is None.
///
/// Example:
///
/// ```
/// use csvlens::{run_csvlens_with_options, CsvlensOptions};
///
/// let options = CsvlensOptions {
///     filename: Some("/path/to/your.csv".to_string()),
///     ..Default::default()
/// };
/// match run_csvlens_with_options(options) {
///     Ok(Some(selected_cell)) => println!("Selected: {}", selected_cell),
///     Ok(None) => {},
///     Err(e) => eprintln!("Error: {:?}", e),
/// }
/// ```
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
        options.freeze_cols_offset,
        options.color_columns,
        options.prompt,
        options.wrap_mode,
    )?;

    let mut app_runner = AppRunner::new(app);
    app_runner.run()
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
    let mut args_items = vec![OsString::from("csvlens")];
    for item in args {
        args_items.push(item.into());
    }
    let args = Args::parse_from(args_items);
    run_csvlens_with_options(args.into())
}

#[cfg(not(feature = "cli"))]
pub fn run_csvlens<I, T>(_args: I) -> CsvlensResult<Option<String>>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    eprintln!("Error: CLI is not enabled. Compile with the 'cli' feature to use this binary.");
    std::process::exit(1);
}
