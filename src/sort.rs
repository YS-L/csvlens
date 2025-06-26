use crate::csv;
use crate::errors::CsvlensResult;

use std::cmp::Ordering;
use std::fs::File;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread::{self};

use arrow::array::{Array, ArrayIter};
use arrow::compute::concat;
use arrow::compute::kernels;
use arrow::datatypes::Fields;
use arrow::datatypes::Schema;
use arrow::datatypes::SchemaBuilder;

#[derive(Clone, Debug, PartialEq)]
pub enum SorterStatus {
    Running,
    Finished,
    Error(String),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SortOrder {
    Ascending,
    Descending,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SortType {
    Lexicographic,
    Natural,
}

// Natural sorting comparison function
fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut a_chars = a.chars().peekable();
    let mut b_chars = b.chars().peekable();

    loop {
        // Skip leading whitespace
        while a_chars.peek().is_some_and(|c| c.is_whitespace()) {
            a_chars.next();
        }
        while b_chars.peek().is_some_and(|c| c.is_whitespace()) {
            b_chars.next();
        }

        // Check if we've reached the end of both strings
        let a_done = a_chars.peek().is_none();
        let b_done = b_chars.peek().is_none();

        if a_done && b_done {
            return Ordering::Equal;
        } else if a_done {
            return Ordering::Less;
        } else if b_done {
            return Ordering::Greater;
        }

        // Check if both characters are digits
        let a_is_digit = a_chars.peek().is_some_and(|c| c.is_ascii_digit());
        let b_is_digit = b_chars.peek().is_some_and(|c| c.is_ascii_digit());

        if a_is_digit && b_is_digit {
            // Both are digits, compare numerically
            let a_num = parse_number(&mut a_chars);
            let b_num = parse_number(&mut b_chars);

            match a_num.cmp(&b_num) {
                Ordering::Equal => continue,
                other => return other,
            }
        } else if a_is_digit {
            // Only a is digit, digits come before non-digits
            return Ordering::Less;
        } else if b_is_digit {
            // Only b is digit, digits come before non-digits
            return Ordering::Greater;
        } else {
            // Both are non-digits, compare lexicographically
            let a_char = a_chars.next().unwrap();
            let b_char = b_chars.next().unwrap();

            match a_char.cmp(&b_char) {
                Ordering::Equal => continue,
                other => return other,
            }
        }
    }
}

fn parse_number(chars: &mut std::iter::Peekable<std::str::Chars>) -> u64 {
    let mut num = 0u64;
    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() {
            num = num * 10 + c.to_digit(10).unwrap() as u64;
            chars.next();
        } else {
            break;
        }
    }
    num
}

#[derive(Debug)]
pub struct Sorter {
    pub column_index: usize,
    column_name: String,
    #[allow(dead_code)]
    sort_type: SortType,
    internal: Arc<Mutex<SorterInternalState>>,
}

impl Sorter {
    pub fn new(
        csv_config: Arc<csv::CsvConfig>,
        column_index: usize,
        column_name: String,
        sort_type: SortType,
    ) -> Self {
        let internal = SorterInternalState::init(csv_config, column_index, sort_type);
        Sorter {
            column_index,
            column_name,
            sort_type,
            internal,
        }
    }

    pub fn get_sorted_indices(
        &self,
        rows_from: u64,
        num_rows: u64,
        order: SortOrder,
    ) -> Option<Vec<u64>> {
        let m_guard = self.internal.lock().unwrap();
        if let Some(sort_result) = &m_guard.sort_result {
            let mut out = vec![];
            let index_range: Box<dyn Iterator<Item = u64>> = if order == SortOrder::Ascending {
                let start = rows_from;
                let end = start.saturating_add(num_rows);
                Box::new(start..end)
            } else {
                let end = sort_result.num_rows() as u64 - rows_from;
                let start = end.saturating_sub(num_rows);
                Box::new((start..end).rev())
            };
            for i in index_range {
                if let Some(record_index) = sort_result.record_indices.get(i as usize) {
                    out.push(*record_index as u64)
                }
            }
            return Some(out);
        }
        None
    }

    pub fn get_record_order(&self, row_index: u64, order: SortOrder) -> Option<u64> {
        let m_guard = self.internal.lock().unwrap();
        if let Some(sort_result) = &m_guard.sort_result {
            if let Some(mut record_order) =
                sort_result.record_orders.get(row_index as usize).cloned()
            {
                if order == SortOrder::Descending {
                    record_order = sort_result.num_rows() - record_order - 1;
                }
                return Some(record_order as u64);
            }
        }
        None
    }

    pub fn status(&self) -> SorterStatus {
        (self.internal.lock().unwrap()).status.clone()
    }

    pub fn column_name(&self) -> &str {
        self.column_name.as_str()
    }

    pub fn terminate(&self) {
        let mut m = self.internal.lock().unwrap();
        m.terminate();
    }

    #[cfg(test)]
    pub fn wait_internal(&self) {
        loop {
            if self.internal.lock().unwrap().done {
                break;
            }
            thread::sleep(core::time::Duration::from_millis(100));
        }
    }
}

impl Drop for Sorter {
    fn drop(&mut self) {
        self.terminate();
    }
}

#[derive(Debug)]
struct SortResult {
    record_indices: Vec<usize>,
    record_orders: Vec<usize>,
}

impl SortResult {
    fn num_rows(&self) -> usize {
        self.record_indices.len()
    }
}

#[derive(Debug)]
struct SorterInternalState {
    sort_result: Option<SortResult>,
    status: SorterStatus,
    should_terminate: bool,
    done: bool,
}

impl SorterInternalState {
    pub fn init(
        config: Arc<csv::CsvConfig>,
        column_index: usize,
        sort_type: SortType,
    ) -> Arc<Mutex<SorterInternalState>> {
        let m_state = Arc::new(Mutex::new(SorterInternalState {
            sort_result: None,
            status: SorterStatus::Running,
            should_terminate: false,
            done: false,
        }));

        let _m = m_state.clone();
        thread::spawn(move || {
            let sort_result = if sort_type == SortType::Natural {
                // Use natural sorting
                run_natural_sort(_m.clone(), config, column_index)
            } else {
                // Use existing lexicographic sorting
                run_lexicographic_sort(_m.clone(), config, column_index)
            };

            let mut m = _m.lock().unwrap();
            if let Ok(sort_result) = sort_result {
                m.sort_result = Some(sort_result);
                m.status = SorterStatus::Finished;
            } else {
                m.status = SorterStatus::Error(sort_result.err().unwrap().to_string());
            }
            m.done = true;
        });

        m_state
    }

    fn infer_schema(filename: &str, delimiter: u8) -> CsvlensResult<Schema> {
        let schema = arrow::csv::infer_schema_from_files(
            &[filename.to_string()],
            delimiter,
            Some(1000),
            true,
        )?;

        // Convert integer fields to float64 to be more permissive
        let mut updated_fields = vec![];
        for field in schema.fields() {
            if field.data_type().is_integer() {
                let new_field = field
                    .as_ref()
                    .clone()
                    .with_data_type(arrow::datatypes::DataType::Float64);
                updated_fields.push(new_field);
            } else {
                updated_fields.push(field.as_ref().clone());
            }
        }
        let updated_fields = Fields::from(updated_fields);

        Ok(SchemaBuilder::from(updated_fields).finish())
    }

    fn terminate(&mut self) {
        self.should_terminate = true;
    }
}

fn run_natural_sort(
    m: Arc<Mutex<SorterInternalState>>,
    config: Arc<csv::CsvConfig>,
    column_index: usize,
) -> CsvlensResult<SortResult> {
    // Read all values and their indices
    let mut values_with_indices: Vec<(String, usize)> = Vec::new();
    let mut reader = config.new_reader()?;

    // Skip header if present
    if config.has_headers() {
        reader.headers()?;
    }

    for (index, result) in reader.records().enumerate() {
        if m.lock().unwrap().should_terminate {
            return Ok(SortResult {
                record_indices: vec![],
                record_orders: vec![],
            });
        }

        let record = result?;
        if let Some(field) = record.get(column_index) {
            values_with_indices.push((field.to_string(), index));
        } else {
            // Handle missing field
            values_with_indices.push(("".to_string(), index));
        }
    }

    // Sort using natural comparison
    values_with_indices.sort_by(|(a, _), (b, _)| natural_cmp(a, b));

    // Construct result
    let mut sorted_record_indices: Vec<usize> = Vec::with_capacity(values_with_indices.len());
    let mut record_orders: Vec<usize> = vec![0; values_with_indices.len()];

    for (order, (_, original_index)) in values_with_indices.into_iter().enumerate() {
        sorted_record_indices.push(original_index);
        record_orders[original_index] = order;
    }

    Ok(SortResult {
        record_indices: sorted_record_indices,
        record_orders,
    })
}

fn run_lexicographic_sort(
    m: Arc<Mutex<SorterInternalState>>,
    config: Arc<csv::CsvConfig>,
    column_index: usize,
) -> CsvlensResult<SortResult> {
    // Existing lexicographic sorting logic
    let schema = SorterInternalState::infer_schema(config.filename(), config.delimiter())?;
    let file = File::open(config.filename())?;
    let arrow_csv_reader = arrow::csv::ReaderBuilder::new(Arc::new(schema))
        .with_delimiter(config.delimiter())
        .with_header(!config.no_headers())
        .with_projection(vec![column_index])
        .build(file)?;

    let mut arrs: Vec<Arc<dyn Array>> = Vec::new();
    for record_batch_result in arrow_csv_reader {
        let record_batch = record_batch_result?;
        let arr = record_batch.column(0);
        arrs.push(arr.clone());
        if m.lock().unwrap().should_terminate {
            return Ok(SortResult {
                record_indices: vec![],
                record_orders: vec![],
            });
        }
    }
    let ref_arrs = arrs
        .iter()
        .map(|arr| arr.as_ref())
        .collect::<Vec<&dyn Array>>();
    let combined_arr = concat(&ref_arrs)?;

    let sorted_indices = kernels::sort::sort_to_indices(combined_arr.as_ref(), None, None)?;

    let mut sorted_record_indices: Vec<usize> = vec![];
    let mut record_orders: Vec<usize> = vec![0; sorted_indices.len()];
    for (record_order, sorted_record_index) in ArrayIter::new(&sorted_indices).flatten().enumerate()
    {
        sorted_record_indices.push(sorted_record_index as usize);
        record_orders[sorted_record_index as usize] = record_order;
    }
    let sort_result = SortResult {
        record_indices: sorted_record_indices,
        record_orders,
    };
    Ok(sort_result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_natural_sort() {
        let mut items = vec!["disk1", "disk10", "disk2", "disk11"];
        items.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(items, vec!["disk1", "disk2", "disk10", "disk11"]);
    }

    #[test]
    fn test_natural_sort_mixed() {
        let mut items = vec!["file1.txt", "file10.txt", "file2.txt", "file20.txt"];
        items.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(
            items,
            vec!["file1.txt", "file2.txt", "file10.txt", "file20.txt"]
        );
    }

    #[test]
    fn test_natural_sort_with_text() {
        let mut items = vec!["chapter1", "chapter10", "chapter2", "chapter20", "appendix"];
        items.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(
            items,
            vec!["appendix", "chapter1", "chapter2", "chapter10", "chapter20"]
        );
    }

    #[test]
    fn test_simple() {
        let config = Arc::new(csv::CsvConfig::new("tests/data/simple.csv", b',', false));
        let s = Sorter::new(config, 0, "A1".to_string(), SortType::Lexicographic);
        s.wait_internal();
        let rows = s.get_sorted_indices(0, 5, SortOrder::Ascending).unwrap();
        let expected = vec![0, 9, 99, 999, 1000];
        assert_eq!(rows, expected);
    }

    #[test]
    fn test_descending() {
        let config = Arc::new(csv::CsvConfig::new("tests/data/simple.csv", b',', false));
        let s = Sorter::new(config, 0, "A1".to_string(), SortType::Lexicographic);
        s.wait_internal();
        let rows = s.get_sorted_indices(0, 5, SortOrder::Descending).unwrap();
        let expected = vec![998, 997, 996, 995, 994];
        assert_eq!(rows, expected);
    }

    #[test]
    fn test_empty() {
        let config = Arc::new(csv::CsvConfig::new("tests/data/empty.csv", b',', false));
        let s = Sorter::new(config, 1, "b".to_string(), SortType::Lexicographic);
        s.wait_internal();
        assert_eq!(
            s.status(),
            SorterStatus::Error("Compute error: Sort not supported for data type Null".to_string())
        );
    }
}
