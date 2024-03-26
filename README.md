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
`Shift + ↓` (or `Shift + j`) | Sort rows by the selected column
`#` (in Cell mode) | Find and highlight rows like the selected cell
`@` (in Cell mode) | Filter rows like the selected cell
`y` (in Cell Mode) | Copy the selected cell to clipboard
`Enter` (in Cell mode) | Print the selected cell to stdout and exit
`-S` | Toggle line wrapping
`-W` | Toggle line wrapping by words
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

* `--filter <regex>`: Use this regex to filter rows to display by default.

* `--find <regex>`: Use this regex to find and highlight matches by default.

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
