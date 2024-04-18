extern crate csv;

use anyhow::Result;
use csv::{Position, Reader, ReaderBuilder};
use std::cmp::max;
use std::fs::File;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

fn string_record_to_vec(record: &csv::StringRecord) -> Vec<String> {
    let mut string_vec = Vec::new();
    for field in record.iter() {
        string_vec.push(String::from(field));
    }
    string_vec
}

pub struct CsvConfig {
    path: String,
    delimiter: u8,
    no_headers: bool,
}

impl CsvConfig {
    pub fn new(path: &str, delimiter: u8, no_headers: bool) -> CsvConfig {
        CsvConfig {
            path: path.to_string(),
            delimiter,
            no_headers,
        }
    }

    pub fn new_reader(&self) -> Result<Reader<File>> {
        let reader = ReaderBuilder::new()
            .flexible(true)
            .delimiter(self.delimiter)
            .has_headers(!self.no_headers)
            .from_path(self.path.as_str())?;
        Ok(reader)
    }

    pub fn filename(&self) -> &str {
        self.path.as_str()
    }

    pub fn delimiter(&self) -> u8 {
        self.delimiter
    }

    pub fn no_headers(&self) -> bool {
        self.no_headers
    }

    pub fn has_headers(&self) -> bool {
        !self.no_headers
    }

    /// Convert position to a 0-based record index
    pub fn position_to_record_index(&self, position: u64) -> u64 {
        if self.no_headers {
            position
        } else {
            position - 1
        }
    }

    /// Convert position to a 1-based record number
    pub fn position_to_record_num(&self, position: u64) -> u64 {
        if self.no_headers {
            position + 1
        } else {
            position
        }
    }
}

pub struct CsvLensReader {
    config: Arc<CsvConfig>,
    reader: Reader<File>,
    pub headers: Vec<String>,
    internal: Arc<Mutex<ReaderInternalState>>,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Row {
    pub record_num: usize,
    pub fields: Vec<String>,
}

impl Row {
    pub fn subset(&self, indices: &[usize]) -> Row {
        let mut subfields = vec![];
        for i in indices {
            subfields.push(self.fields.get(*i).unwrap().clone());
        }
        Row {
            record_num: self.record_num,
            fields: subfields,
        }
    }

    fn empty() -> Row {
        Row {
            record_num: 0,
            fields: vec![],
        }
    }
}

#[derive(Debug)]
struct GetRowIndex {
    // 0-based index of the record in the csv file
    record_index: u64,

    // Position where the record should be in the resulting list of rows
    order_index: usize,
}

impl CsvLensReader {
    pub fn new(config: Arc<CsvConfig>) -> Result<Self> {
        let mut reader = config.new_reader()?;

        let headers_record = if config.no_headers() {
            let mut dummy_headers = csv::StringRecord::new();
            for (i, _) in reader.headers()?.into_iter().enumerate() {
                dummy_headers.push_field((i + 1).to_string().as_str());
            }
            dummy_headers
        } else {
            reader.headers()?.clone()
        };
        let headers = string_record_to_vec(&headers_record);

        let (m_internal, _handle) = ReaderInternalState::init_internal(config.clone());

        let reader = Self {
            config: config.clone(),
            reader,
            headers,
            internal: m_internal,
        };
        Ok(reader)
    }

    pub fn get_rows(&mut self, rows_from: u64, num_rows: u64) -> Result<Vec<Row>> {
        let indices: Vec<u64> = (rows_from..rows_from + num_rows).collect();
        self.get_rows_impl(&indices).map(|x| x.0)
    }

    pub fn get_rows_for_indices(&mut self, indices: &[u64]) -> Result<Vec<Row>> {
        self.get_rows_impl(indices).map(|x| x.0)
    }

    fn get_rows_impl(&mut self, indices: &[u64]) -> Result<(Vec<Row>, GetRowsStats)> {
        let mut get_row_indices = indices
            .iter()
            .enumerate()
            .map(|x| GetRowIndex {
                record_index: *x.1,
                order_index: x.0,
            })
            .collect::<Vec<_>>();
        get_row_indices.sort_by(|a, b| a.record_index.cmp(&b.record_index));
        self._get_rows_impl_sorted(&get_row_indices)
    }

    fn _get_rows_impl_sorted(
        &mut self,
        indices: &[GetRowIndex],
    ) -> Result<(Vec<Row>, GetRowsStats)> {
        // stats for debugging and testing
        let mut stats = GetRowsStats::new();

        let pos = Position::new();
        self.reader.seek(pos)?;

        let pos_table = self.get_pos_table();
        let mut pos_iter = pos_table.iter();
        let mut indices_iter = indices.iter();

        let mut res = vec![Row::empty(); indices.len()];
        let mut res_max_index: Option<usize> = None;

        let mut next_pos = pos_iter.next();
        let mut next_wanted = indices_iter.next();
        loop {
            if next_wanted.is_none() {
                break;
            }
            // seek as close to the next wanted record index as possible
            let index = next_wanted.unwrap();
            while let Some(pos) = next_pos {
                if self.config.position_to_record_index(pos.record()) <= index.record_index {
                    self.reader.seek(pos.clone())?;
                    stats.log_seek();
                } else {
                    break;
                }
                next_pos = pos_iter.next();
            }

            // note that records() excludes header by default, but here the first entry is header
            // because of the seek() above.
            let mut records = self.reader.records();

            // parse records and collect those that are wanted
            loop {
                // exit early if all found. This should be common in case of consecutive indices
                if next_wanted.is_none() {
                    break;
                }
                let wanted = next_wanted.unwrap();
                let record_position = records.reader().position().record();
                if let Some(r) = records.next() {
                    stats.log_parsed_record();
                    // no effective pre-seeking happened, this is still the header
                    if self.config.has_headers() && record_position == 0 {
                        continue;
                    }
                    if self.config.position_to_record_index(record_position) == wanted.record_index
                    {
                        let string_record = r?;
                        let mut fields = Vec::new();
                        for field in string_record.iter() {
                            fields.push(String::from(field));
                        }
                        let row = Row {
                            record_num: self.config.position_to_record_num(record_position)
                                as usize,
                            fields,
                        };
                        res[wanted.order_index] = row;
                        res_max_index.replace(
                            res_max_index
                                .map_or(wanted.order_index, |x| max(x, wanted.order_index)),
                        );
                        next_wanted = indices_iter.next();
                    }
                    // stop parsing if done scanning whole block between marked positions
                    if let Some(pos) = next_pos {
                        if record_position >= pos.record() {
                            break;
                        }
                    }
                } else {
                    // no more records
                    break;
                }
            }

            if next_pos.is_none() {
                // If here, the last block had been scanned, and we should be
                // done. If next_wanted is not None, that means an out of bound
                // index was provided - that could happen for small input - and
                // we should ignore it and stop here regardless
                break;
            }
        }

        // In case requested indices are beyond the last record, truncate those indices.
        res.truncate(res_max_index.map_or(0, |x| x + 1));

        Ok((res, stats))
    }

    pub fn get_total_line_numbers(&self) -> Option<usize> {
        let res = self.internal.lock().unwrap().total_line_number;
        res
    }

    pub fn get_last_indexed_line_number(&self) -> Option<usize> {
        let res = self
            .internal
            .lock()
            .unwrap()
            .pos_table
            .last()
            .map(|x| x.record() as usize);
        res
    }

    pub fn get_pos_table(&self) -> Vec<Position> {
        let res = self.internal.lock().unwrap().pos_table.clone();
        res
    }
}

#[derive(Debug, PartialEq)]
struct GetRowsStats {
    num_seek: u64,
    num_parsed_record: u64,
}

impl GetRowsStats {
    fn new() -> GetRowsStats {
        GetRowsStats {
            num_seek: 0,
            num_parsed_record: 0,
        }
    }

    fn log_seek(&mut self) {
        self.num_seek += 1;
    }

    fn log_parsed_record(&mut self) {
        self.num_parsed_record += 1
    }
}

struct ReaderInternalState {
    total_line_number: Option<usize>,
    pos_table: Vec<Position>,
    done: bool,
}

impl ReaderInternalState {
    fn init_internal(config: Arc<CsvConfig>) -> (Arc<Mutex<ReaderInternalState>>, JoinHandle<()>) {
        let internal = ReaderInternalState {
            total_line_number: None,
            pos_table: vec![],
            done: false,
        };

        let m_state = Arc::new(Mutex::new(internal));

        let _m = m_state.clone();
        let handle = thread::spawn(move || {
            let filesize = File::open(config.filename())
                .unwrap()
                .metadata()
                .unwrap()
                .len();
            let pos_table_num_entries = 10000;
            let minimum_interval = 500; // handle small csv (don't keep pos every byte)
            let pos_table_update_every = max(minimum_interval, filesize / pos_table_num_entries);

            // full csv parsing
            let bg_reader = config.new_reader().unwrap();
            let mut n_lines = 0;
            let mut n_bytes: u64 = 0;
            let mut last_updated_at = 0;
            let mut iter = bg_reader.into_records();
            loop {
                let next_pos = iter.reader().position().clone();
                if iter.next().is_none() {
                    break;
                }
                // must not include headers position here (n > 0)
                let cur = (n_bytes / pos_table_update_every) as u64;
                if n_bytes > 0 && cur > last_updated_at {
                    let mut m = _m.lock().unwrap();
                    m.pos_table.push(next_pos.clone());
                    last_updated_at = cur;
                }
                n_lines += 1;
                n_bytes = next_pos.byte();
            }
            let mut m = _m.lock().unwrap();
            m.total_line_number = Some(n_lines);
            m.done = true;
        });

        (m_state, handle)
    }
}

#[cfg(test)]
mod tests {
    use core::time;

    use super::*;

    impl Row {
        pub fn new(record_num: usize, fields: Vec<&str>) -> Row {
            Row {
                record_num,
                fields: fields.iter().map(|x| x.to_string()).collect(),
            }
        }
    }

    impl CsvLensReader {
        fn wait_internal(&self) {
            loop {
                if self.internal.lock().unwrap().done {
                    break;
                }
                thread::sleep(time::Duration::from_millis(100));
            }
        }
    }

    #[test]
    fn test_cities_get_rows() {
        let config = Arc::new(CsvConfig::new("tests/data/cities.csv", b',', false));
        let mut r = CsvLensReader::new(config).unwrap();
        r.wait_internal();
        let rows = r.get_rows(2, 3).unwrap();
        let expected = vec![
            Row::new(
                3,
                vec![
                    "46", "35", "59", "N", "120", "30", "36", "W", "Yakima", "WA",
                ],
            ),
            Row::new(
                4,
                vec![
                    "42",
                    "16",
                    "12",
                    "N",
                    "71",
                    "48",
                    "0",
                    "W",
                    "Worcester",
                    "MA",
                ],
            ),
            Row::new(
                5,
                vec![
                    "43",
                    "37",
                    "48",
                    "N",
                    "89",
                    "46",
                    "11",
                    "W",
                    "Wisconsin Dells",
                    "WI",
                ],
            ),
        ];
        assert_eq!(rows, expected);
    }

    #[test]
    fn test_simple_get_rows() {
        let config = Arc::new(CsvConfig::new("tests/data/simple.csv", b',', false));
        let mut r = CsvLensReader::new(config).unwrap();
        r.wait_internal();
        let rows = r.get_rows(1234, 2).unwrap();
        let expected = vec![
            Row::new(1235, vec!["A1235", "B1235"]),
            Row::new(1236, vec!["A1236", "B1236"]),
        ];
        assert_eq!(rows, expected);
    }

    #[test]
    fn test_simple_get_rows_out_of_bound() {
        let config = Arc::new(CsvConfig::new("tests/data/simple.csv", b',', false));
        let mut r = CsvLensReader::new(config).unwrap();
        r.wait_internal();
        let indices = vec![5000];
        let (rows, _stats) = r.get_rows_impl(&indices).unwrap();
        assert_eq!(rows, vec![]);
    }

    #[test]
    fn test_simple_get_rows_impl_1() {
        let config = Arc::new(CsvConfig::new("tests/data/simple.csv", b',', false));
        let mut r = CsvLensReader::new(config).unwrap();
        r.wait_internal();
        let indices = vec![1, 3, 5, 1234, 2345, 3456, 4999];
        let (rows, stats) = r.get_rows_impl(&indices).unwrap();
        let expected = vec![
            Row::new(2, vec!["A2", "B2"]),
            Row::new(4, vec!["A4", "B4"]),
            Row::new(6, vec!["A6", "B6"]),
            Row::new(1235, vec!["A1235", "B1235"]),
            Row::new(2346, vec!["A2346", "B2346"]),
            Row::new(3457, vec!["A3457", "B3457"]),
            Row::new(5000, vec!["A5000", "B5000"]),
        ];
        assert_eq!(rows, expected);
        let expected = GetRowsStats {
            num_seek: 115,
            num_parsed_record: 218,
        };
        assert_eq!(stats, expected);
    }

    #[test]
    fn test_simple_get_rows_impl_2() {
        let config = Arc::new(CsvConfig::new("tests/data/simple.csv", b',', false));
        let mut r = CsvLensReader::new(config).unwrap();
        r.wait_internal();
        let indices = vec![1234];
        let (rows, stats) = r.get_rows_impl(&indices).unwrap();
        let expected = vec![Row::new(1235, vec!["A1235", "B1235"])];
        assert_eq!(rows, expected);
        let expected = GetRowsStats {
            num_seek: 25,
            num_parsed_record: 8,
        };
        assert_eq!(stats, expected);
    }

    #[test]
    fn test_simple_get_rows_impl_3() {
        let config = Arc::new(CsvConfig::new("tests/data/simple.csv", b',', false));
        let mut r = CsvLensReader::new(config).unwrap();
        r.wait_internal();
        let indices = vec![2];
        let (rows, stats) = r.get_rows_impl(&indices).unwrap();
        let expected = vec![Row::new(3, vec!["A3", "B3"])];
        assert_eq!(rows, expected);
        let expected = GetRowsStats {
            num_seek: 0,
            num_parsed_record: 4, // 3 + 1 (including header)
        };
        assert_eq!(stats, expected);
    }

    #[test]
    fn test_small() {
        let config = Arc::new(CsvConfig::new("tests/data/small.csv", b',', false));
        let mut r = CsvLensReader::new(config).unwrap();
        let rows = r.get_rows(0, 50).unwrap();
        let expected = vec![
            Row::new(1, vec!["c1", " v1"]),
            Row::new(2, vec!["c2", " v2"]),
        ];
        assert_eq!(rows, expected);
    }

    #[test]
    fn test_small_delimiter() {
        let config = Arc::new(CsvConfig::new("tests/data/small.bsv", b'|', false));
        let mut r = CsvLensReader::new(config).unwrap();
        let rows = r.get_rows(0, 50).unwrap();
        let expected = vec![Row::new(1, vec!["c1", "v1"]), Row::new(2, vec!["c2", "v2"])];
        assert_eq!(rows, expected);
    }

    #[test]
    fn test_irregular() {
        let config = Arc::new(CsvConfig::new("tests/data/irregular.csv", b',', false));
        let mut r = CsvLensReader::new(config).unwrap();
        let rows = r.get_rows(0, 50).unwrap();
        let expected = vec![Row::new(1, vec!["c1"]), Row::new(2, vec!["c2", " v2"])];
        assert_eq!(rows, expected);
    }

    #[test]
    fn test_double_quoting_as_escape_chars() {
        let config = Arc::new(CsvConfig::new(
            "tests/data/good_double_quote.csv",
            b',',
            false,
        ));
        let mut r = CsvLensReader::new(config).unwrap();
        let rows = r.get_rows(0, 50).unwrap();
        let expected = vec![
            Row::new(1, vec!["1", "quote"]),
            Row::new(2, vec!["5", "Comma, comma"]),
        ];
        assert_eq!(rows, expected);
    }

    #[test]
    fn get_rows_unsorted_indices() {
        let config = Arc::new(CsvConfig::new("tests/data/simple.csv", b',', false));
        let mut r = CsvLensReader::new(config).unwrap();
        r.wait_internal();
        let rows = r.get_rows_for_indices(&vec![1235, 1234]).unwrap();
        let expected = vec![
            Row::new(1236, vec!["A1236", "B1236"]),
            Row::new(1235, vec!["A1235", "B1235"]),
        ];
        assert_eq!(rows, expected);
    }
}
