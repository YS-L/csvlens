# v0.1.13

* Switch to Rust 2021 edition and update dependencies (#25)
* Fix crossterm panic by upgrading to version 0.26.1
* New styling for selected row

# v0.1.12

* Add --version option
* Add --echo-column option to print column's value at selected row to stdout
* Use stderr as tui buffer to support piping from csvlens

# v0.1.11

* Attempt to restore terminal state on panic
* Fix piped input not working on macOS

# v0.1.10

* Handle irregular CSV when calculating column widths
* Improved event loop handling
* Improved memory usage when creating temporary file from stdin

# v0.1.9

* Support filtering on columns
* Support basic command history

# v0.1.8

* Support horizontal scrolling

# v0.1.7

* Ensure terminal state is restored on error

# v0.1.6

* Fix bug where program sometimes crashes due to unicode characters
* Switch to `crossterm`

# v0.1.5

* Support irregular CSV to some extent (parse CSV in non-strict mode)
* Support regex patterns in search and filter
* Support scrolling to top with `g`