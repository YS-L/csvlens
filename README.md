# csvlens

`csvlens` is a command line CSV file viewer. It is like `less` but made
for CSV.

![Demo](.github/demo.gif)

## Usage

Run `csvlens` by providing the CSV filename:

```
csvlens <filename>
```

Pipe CSV data directly to `csvlens`:

```
<your commands producing some csv data> | csvlens
```
### Key bindings

Key | Action
--- | ---
`hjkl` (or `← ↓ ↑→ `) | Scroll one row or column in the given direction
`Ctrl + f` (or `Page Down`) | Scroll one window down
`Ctrl + b` (or `Page Up`) | Scroll one window up
`Ctrl + d` (or `d`) | Scroll half a window down
`Ctrl + u` (or `u`) | Scroll half a window up
`Ctrl + h` | Scroll one window left
`Ctrl + l` | Scroll one window right
`Ctrl + ←` | Scroll left to first column
`Ctrl + →` | Scroll right to last column
`Ctrl + e` | Print the marked lines to stdout and exit
`G` (or `End`) | Go to bottom
`g` (or `Home`) | Go to top
`<n>G` | Go to line `n`
`/<regex>` | Find content matching regex and highlight matches
`n` (in Find mode) | Jump to next result
`N` (in Find mode) | Jump to previous result
`&<regex>` | Filter rows using regex (show only matches)
`*<regex>` | Filter columns using regex (show only matches)
`TAB` | Toggle between row, column or cell selection modes
`>` | Increase selected column's width
`<` | Decrease selected column's width
`Shift + ↓` (or `J`) | Sort rows or toggle sort direction by the selected column
`Ctrl + j` | Same as above, but sort by natural ordering (e.g. "file2" < "file10")
`#` (in Cell mode) | Find and highlight rows like the selected cell
`@` (in Cell mode) | Filter rows like the selected cell
`y` | Copy the selected row or cell to clipboard
`Enter` (in Cell mode) | Print the selected cell to stdout and exit
`-S` | Toggle line wrapping
`-W` | Toggle line wrapping by words
`f<n>` | Freeze this number of columns from the left
`m` | Mark / unmark the selected row visually
`M` | Clear all row marks
`r` | Reset to default view (clear all filters and custom column widths)
`H` (or `?`) | Display help
`q` | Exit

### Optional parameters

* `-d <char>`: Use this delimiter when parsing the CSV
  (e.g. `csvlens file.csv -d '\t'`).

  Specify `-d auto` to auto-detect the delimiter.

* `-t`, `--tab-separated`: Use tab as the delimiter (when specified, `-d` is ignored).

* `-i`, `--ignore-case`: Ignore case when searching. This flag is ignored if any
  uppercase letters are present in the search string.

* `--no-headers`: Do not interpret the first row as headers.

* `--columns <regex>`: Use this regex to select columns to display by default.

  Example: `"column1|column2"` matches `"column1"`, `"column2"`, and also column names like
  `"column11"`, `"column22"`.

* `--filter <regex>`: Use this regex to filter rows to display by default.

  The regex is matched against each cell in every column.

  Example: `"value1|value2"` filters rows with any cells containing `"value1"`, `"value2"`, or text
  like `"my_value1"` or `"value234"`.

* `--find <regex>`: Use this regex to find and highlight matches by default.

  The regex is matched against each cell in every column.

  Example: `"value1|value2"` highlights text in any cells containing `"value1"`, `"value2"`, or
  longer text like `"value1_ok"`.

* `--echo-column <column_name>`: Print the value of this column at the selected
  row to stdout on `Enter` key and then exit.

* `--prompt <prompt>`: Show a custom prompt message in the status bar. Supports ANSI escape codes
  for colored or styled text.

  Example:
  ```bash
  csvlens Pokemon.csv --prompt $'\e[1m\e[32mSelect a Pokémon!\e[0m'
  ```

* `--color-columns` (or `--colorful`): Display each column in a different color.

## Installation

### Direct download

You can download the `tar.xz` or `zip` file matching your operating system from the
[releases page](https://github.com/YS-L/csvlens/releases), extract it and execute the `csvlens`
binary.

### Homebrew

For macOS, `csvlens` is available on [Homebrew](https://formulae.brew.sh/formula/csvlens). You can
install it using:
```
brew install csvlens
```

### Arch Linux
`csvlens` is available in the [official repositories](https://archlinux.org/packages/extra/x86_64/csvlens). You can install it using:
```
pacman -S csvlens
```

### Windows

For Windows, `csvlens` is available on [winget](https://learn.microsoft.com/en-gb/windows/package-manager/). You can install it using:
```powershell
winget install --id YS-L.csvlens
```

### FreeBSD
`csvlens` is available as a [FreeBSD pkg](https://www.freshports.org/textproc/csvlens/). You can install it using:
```
pkg install csvlens
```

### NetBSD
`csvlens` is available on [pkgsrc](https://ftp.netbsd.org/pub/pkgsrc/current/pkgsrc/textproc/csvlens/index.html). If you're using NetBSD you can install it using:
```
pkgin install csvlens
```

### OpenBSD
`csvlens` is available as an [OpenBSD port](https://cvsweb.openbsd.org/ports/textproc/csvlens/). If you're using OpenBSD 7.6-current or later, you can install it using:
```
doas pkg_add csvlens
```

### Cargo

If you have [Rust](https://www.rust-lang.org/tools/install) installed, `csvlens` is available on
[crates.io](https://crates.io/crates/csvlens) and you can install it using:
```
cargo install csvlens
```

Or, build and install from source after cloning this repo:
```
cargo install --path $(pwd)
```

Requires Rust 1.88.0 or newer.

## Library Usage

This crate allows you to use csvlens as a library.

In your `Cargo.toml`, add the following:

```toml
[dependencies]
csvlens = { version = "0.12.0", default-features = false, features = ["clipboard"] }
```

### Example

Here's a simple example of how to use `csvlens` as a library ([Documentation](https://docs.rs/csvlens/0.12.0/csvlens/index.html)):

```rust
use csvlens::run_csvlens;

let out = run_csvlens(&["/path/to/your.csv"]).unwrap();
if let Some(selected_cell) = out {
    println!("Selected: {}", selected_cell);
}
```

For more advanced usage, you can use `CsvlensOptions` to customize the behavior:

```rust
use csvlens::{run_csvlens_with_options, CsvlensOptions};

let options = CsvlensOptions {
    filename: "/path/to/your.csv".to_string(),
    delimiter: Some("|".to_string()),
    ignore_case: true,
    debug: true,
    ..Default::default()
};
let out = run_csvlens_with_options(options).unwrap();
if let Some(selected_cell) = out {
    println!("Selected: {}", selected_cell);
}
```

See how [qsv](https://github.com/dathere/qsv/tree/master?tab=readme-ov-file#qsv-blazing-fast-data-wrangling-toolkit) uses `csvlens` as a library [here](https://github.com/dathere/qsv/blob/master/src/cmd/lens.rs#L2).
