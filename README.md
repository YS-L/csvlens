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
`Ctrl + h` (or `Ctrl + ←`) | Scroll one window left
`Ctrl + l` (or `Ctrl + →`) | Scroll one window right
`G` | Go to bottom
`g` | Go to top
`<n>G` | Go to line `n`
`/<regex>` | Find content matching regex and highlight matches
`n` (in Find mode) | Jump to next result
`N` (in Find mode) | Jump to previous result
`&<regex>` | Filter rows using regex (show only matches)
`*<regex>` | Filter columns using regex (show only matches)
`q` | Exit

### Optional parameters
* `-d <delimiter>`: Custom delimiter to use when parsing the CSV
   (e.g. `csvlens file.csv -d $\t`). Specify `-d auto` to auto-detect the delimiter.
* `-i, --ignore-case`: Searches ignore case. Ignored if any uppercase letters are present in the search string.

## Installation

`csvlens` is available on [crates.io](https://crates.io/crates/csvlens), so you
can install it using:
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
