# csvlens

`csvlens` is a command line CSV file viewer. It is similar to `less` but made
for CSV.

![Demo](.github/demo.gif)

## Usage

Run `csvlens` by providing the CSV filename:

```
csvlens <filename>
```

Pipe CSV data directly to `csvlens`t

```
<your commands producing some csv data> | csvlens
```
### Supported interactions
* Scroll: `hjkl`, `← ↓ ↑→ `, `Page Up`, `Page Down`
* Horizontal scrolling: `Ctrl` + `← →` or `hl`
* Go to bottom: `G`
* Go to top: `g`
* Go to line `n`: `nG`
* Search: `/<thing>`
    * Go to next result: `n`
    * Go to previous result: `N`
* Filter: `&<thing>` (or `//<thing>`)
* Exit: `q`

### Optional parameters
* `-d <delimiter>`: Custom delimiter to use when parsing the CSV
   (e.g. `csvlens file.csv -d \t`)

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
