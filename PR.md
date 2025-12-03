# Copy column to clipboard

This PR implements the functionality to copy column values to the clipboard when a column is selected.

## Features

### 1. Copy column values
- When a column is selected (using `Tab` to switch to column selection mode), pressing `y` will copy the values of that column to the clipboard.
- **Filtered Rows**: If a filter is active, only the filtered rows are copied.
- **All Rows**: If no filter is active, all rows in the column are copied.
- **Sort Order**: The copied values respect the current sort order.

### 2. Clipboard limit
To prevent performance issues and clipboard overflows with large datasets, a limit is applied to the number of rows copied.
- **Default Limit**: 10,000 rows.
- **CLI Configuration**: The limit can be configured using the new `--clipboard-limit` command-line argument.
  ```bash
  csvlens data.csv --clipboard-limit 50000
  ```

### 3. Copy column status feedback
The status message displayed after copying a column to the clipboard:
- Displays the **name** of the copied column.
- Displays the **number of rows** copied.
- Indicates if the output was truncated due to the limit.

