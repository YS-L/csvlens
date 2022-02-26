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
```
cargo install --path $(pwd)
```