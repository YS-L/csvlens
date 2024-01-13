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
`Enter` (in Cell mode) | Print the selected cell to stdout and exit
`-S` | Toggle line wrapping
`r` | Reset to default view (clear all filters and custom column widths)
`H` | Display help
`q` | Exit

### Optional parameters

* `-d <delimiter>`: Use this delimiter when parsing the CSV
  (e.g. `csvlens file.csv -d '\t'`).

  Specify `-d auto` to auto-detect the delimiter.

* `-t`, `--tab-separated`: Use tab as the delimiter (when specified, `-d` is ignored).

* `-i`, `--ignore-case`: Ignore case when searching. This flag is ignored if any
  uppercase letters are present in the search string.

* `--echo-column <column_name>`: Print the value of this column at the selected
  row to stdout on `Enter` key and then exit.

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

### Cargo

If you have [Rust](https://www.rust-lang.org/tools/install) installed, `csvlens` is available on
[crates.io](https://crates.io/crates/csvlens) and you can install it using:
```
cargo install csvlens
```

Or, build and install from source:
```
cargo install --path $(pwd)
```

### Linux package managers
`csvlens` is also available on pkgsrc. If you're using NetBSD you can install it using:
```
pkgin install csvlens
```

### Windows

For Windows, `csvlens` is available on [winget](https://learn.microsoft.com/en-gb/windows/package-manager/).
You can install it using:
```powershell
winget install --id YS-L.csvlens
```
