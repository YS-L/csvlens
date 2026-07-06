use crate::app::{App, WrapMode};
use crate::delimiter::Delimiter;
use crate::errors::CsvlensResult;

#[cfg(feature = "cli")]
use clap::ArgGroup;
#[cfg(feature = "cli")]
use clap::Parser;
#[cfg(feature = "cli")]
use clap::ValueEnum;
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
#[derive(Debug, Clone, ValueEnum)]
#[clap(rename_all = "lower")]
pub enum ClapWrapMode {
    Chars,
    Words,
}

#[cfg(feature = "cli")]
#[derive(Parser, Debug)]
#[command(version)]
#[command(group(ArgGroup::new("wrap_flags").conflicts_with("wrap")))]
#[command(styles = clap_cargo::style::CLAP_STYLING)]
struct Args {
    /// CSV filename
    filename: Option<String>,

    /// Delimiter character (comma by default) or "auto" to auto-detect the delimiter
    #[clap(short, long, value_name = "char")]
    delimiter: Option<String>,

    /// Use tab separation. Shortcut for -d '\t'.
    #[clap(short = 't', long)]
    tab_separated: bool,

    /// Use comma separation. Shortcut for -d ','.
    #[clap(short = 'c', long)]
    comma_separated: bool,

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

    /// Set wrapping mode
    #[arg(long, value_enum, value_name = "mode")]
    pub wrap: Option<ClapWrapMode>,

    /// Shortcut for --wrap=chars (wrap by character count)
    #[arg(short = 'S', group = "wrap_flags")]
    pub wrap_chars: bool,

    /// Shortcut for --wrap=words (wrap by word boundaries)
    #[arg(short = 'W', group = "wrap_flags")]
    pub wrap_words: bool,

    /// Auto-reload the file when it changes on disk
    #[clap(long)]
    pub auto_reload: bool,

    /// Show stats for debugging
    #[clap(long)]
    debug: bool,

    /// Disable streaming stdin (load entire input before displaying)
    #[clap(long)]
    pub no_streaming_stdin: bool,

    /// Format numbers in all columns using the specified format.
    ///
    /// Supported formats: thousands, scientific, si, fixed
    ///
    /// Example: --number-format fixed
    #[arg(long, value_name = "FORMAT")]
    pub number_format: Option<String>,

    /// Format numbers in a specific column.
    ///
    /// Format: COL=FMT where FMT is one of: thousands, scientific, si, fixed
    /// Can be specified multiple times for different columns.
    ///
    /// Example: --column-format "price=thousands" --column-format "value=fixed"
    #[arg(long, value_name = "COL=FMT")]
    pub column_format: Vec<String>,

    /// Number of decimal places for fixed/scientific/si formats (default: 2).
    /// Applies to all columns. Requires --number-format or --column-format to have any effect.
    ///
    /// Example: --number-format fixed --precision 4
    #[arg(long, value_name = "N")]
    pub precision: Option<usize>,
}

#[cfg(feature = "cli")]
impl Args {
    fn get_wrap_mode(
        wrap: Option<ClapWrapMode>,
        wrap_chars: bool,
        wrap_words: bool,
    ) -> Option<WrapMode> {
        if let Some(mode) = wrap {
            return match mode {
                ClapWrapMode::Chars => Some(WrapMode::Chars),
                ClapWrapMode::Words => Some(WrapMode::Words),
            };
        } else {
            if wrap_chars {
                return Some(WrapMode::Chars);
            }
            if wrap_words {
                return Some(WrapMode::Words);
            }
        }
        None
    }
}

#[cfg(feature = "cli")]
fn parse_column_format_config(
    column_format: &[String],
    number_format: Option<&str>,
    precision: Option<usize>,
) -> Option<crate::format::ColumnFormatConfig> {
    use crate::format::{ColumnFormatConfig, NumberFormat};

    let mut config = ColumnFormatConfig::new();

    // Parse global format
    if let Some(fmt_str) = number_format {
        if let Some(fmt) = NumberFormat::from_str(fmt_str) {
            config.set_global(fmt);
        } else {
            eprintln!("Warning: unknown number format '{}', ignoring", fmt_str);
        }
    }

    // Parse per-column format "col=fmt"
    for s in column_format {
        if let Some((col, fmt_str)) = s.split_once('=') {
            if let Some(fmt) = NumberFormat::from_str(fmt_str) {
                config.insert_named(col.to_string(), fmt);
            } else {
                eprintln!("Warning: unknown column format '{}' for column '{}', ignoring", fmt_str, col);
            }
        } else {
            eprintln!("Warning: invalid --column-format '{}', expected COL=FMT format, ignoring", s);
        }
    }

    // Warn early (before set_precision) so config.is_empty() still reflects format-only state.
    if precision.is_some() && number_format.is_none() && column_format.is_empty() {
        eprintln!("Warning: --precision has no effect without --number-format or --column-format");
    }

    if let Some(p) = precision {
        config.set_precision(p);
    }

    if config.is_empty() { None } else { Some(config) }
}

#[cfg(feature = "cli")]
impl From<Args> for CsvlensOptions {
    fn from(args: Args) -> Self {
        let column_format_config = parse_column_format_config(
            &args.column_format,
            args.number_format.as_deref(),
            args.precision,
        );
        Self {
            filename: args.filename,
            delimiter: args.delimiter,
            tab_separated: args.tab_separated,
            comma_separated: args.comma_separated,
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
            wrap_mode: Args::get_wrap_mode(args.wrap, args.wrap_chars, args.wrap_words),
            auto_reload: args.auto_reload,
            no_streaming_stdin: args.no_streaming_stdin,
            column_format_config,
        }
    }
}

// Struct for library usage without clap directives
#[derive(Debug, Default)]
pub struct CsvlensOptions {
    pub filename: Option<String>,
    pub delimiter: Option<String>,
    pub tab_separated: bool,
    pub comma_separated: bool,
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
    pub auto_reload: bool,
    pub no_streaming_stdin: bool,
    pub column_format_config: Option<crate::format::ColumnFormatConfig>,
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
    let delimiter = Delimiter::from_arg(
        &options.delimiter,
        options.tab_separated,
        options.comma_separated,
    )?;

    let app = App::new(
        options.filename,
        delimiter,
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
        options.auto_reload,
        options.no_streaming_stdin,
        options.column_format_config,
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
