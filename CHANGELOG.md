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