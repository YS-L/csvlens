# csvlens

`csvlens` is a CSV file viewer in the command line. It is similar to `less` but
made for CSV.

![Demo](.github/demo.gif)

## Usage

```
csvlens <filename>
```

Supported interactions:
* Scroll: `hjkl`, `← ↓ ↑→ `, `Page Up`, `Page Down`
* Jump to line `n`: `nG`
* Search: `/<thing>`
* Filter: `&<thing>` (or `//<thing>`)

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