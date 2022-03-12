# csvlens

`csvlens` is a CSV file viewer in the command line. It is similar to `less` but
made for CSV.

![Demo](.github/demo.gif)

## Usage

```
csvlens <filename>
```

### Supported interactions
* Scroll: `hjkl`, `← ↓ ↑→ `, `Page Up`, `Page Down`
* Jump to line `n`: `nG`
* Search: `/<thing>`
    * Go to next result: `n`
    * Go to previous result: `N`
* Filter: `&<thing>` (or `//<thing>`)

### Optional parameters
* `-d <delimiter>`: Custom delimiter to use when parsing the CSV
   (e.g. `csvlens file.csv -d \t`)

### Combining with other tools
You can combine `csvlens` with other CSV processing tools, but there is a gotcha
for now: piping data to `csvlens` doesn't work yet, because stdin is reserved
for getting interactive key inputs. You can use process substitution:
```
csvlens <([your commands producing some csv data])
```
For example:
```
csvlens <(xsv frequency data.csv)
```

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