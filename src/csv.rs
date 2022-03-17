extern crate csv;

use anyhow::Result;
use csv::{Position, Reader, ReaderBuilder};
use std::cmp::max;
use std::fs::File;
use std::io::{BufRead, BufReader};
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
    pub delimiter: u8,
}

impl CsvConfig {
    pub fn new(path: &str) -> CsvConfig {
        CsvConfig {
            path: path.to_string(),
            delimiter: b',',
        }
    }

    pub fn new_reader(&self) -> Result<Reader<File>> {
        let reader = ReaderBuilder::new()
            .flexible(true)
            .delimiter(self.delimiter)
            .from_path(self.path.as_str())?;
        Ok(reader)
    }

    pub fn filename(&self) -> &str {
        self.path.as_str()
    }
}

pub struct CsvLensReader {
    reader: Reader<File>,
    pub headers: Vec<String>,
    internal: Arc<Mutex<ReaderInternalState>>,
}

#[derive(Debug, PartialEq)]
pub struct Row {
    pub record_num: usize,
    pub fields: Vec<String>,
}

impl CsvLensReader {
    pub fn new(config: Arc<CsvConfig>) -> Result<Self> {
        let mut reader = config.new_reader()?;
        let headers_record = reader.headers().unwrap();
        let headers = string_record_to_vec(headers_record);

        let (m_internal, _handle) = ReaderInternalState::init_internal(config);

        let reader = Self {
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
        // stats for debugging and testing
        let mut stats = GetRowsStats::new();

        let pos = Position::new();
        self.reader.seek(pos)?;

        let pos_table = self.get_pos_table();
        let mut pos_iter = pos_table.iter();
        let mut indices_iter = indices.iter();

        let mut res = Vec::new();

        let mut next_pos = pos_iter.next();
        let mut next_wanted = indices_iter.next();
        loop {
            if next_wanted.is_none() {
                break;
            }
            // seek as close to the next wanted record index as possible
            let index = *next_wanted.unwrap();
            while let Some(pos) = next_pos {
                if pos.record() - 1 <= index {
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
                let wanted_index = *next_wanted.unwrap();
                let record_num = records.reader().position().record();
                if let Some(r) = records.next() {
                    stats.log_parsed_record();
                    // no effective pre-seeking happened, this is still the header
                    if record_num == 0 {
                        continue;
                    }
                    if record_num - 1 == wanted_index {
                        let string_record = r?;
                        let mut fields = Vec::new();
                        for field in string_record.iter() {
                            fields.push(String::from(field));
                        }
                        let row = Row {
                            record_num: record_num as usize,
                            fields,
                        };
                        res.push(row);
                        next_wanted = indices_iter.next();
                    }
                    // stop parsing if done scanning whole block between marked positions
                    if let Some(pos) = next_pos {
                        if record_num >= pos.record() {
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

        Ok((res, stats))
    }

    pub fn get_total_line_numbers(&self) -> Option<usize> {
        let res = (*self.internal.lock().unwrap()).total_line_number;
        res
    }

    pub fn get_total_line_numbers_approx(&self) -> Option<usize> {
        let res = (*self.internal.lock().unwrap()).total_line_number_approx;
        res
    }

    pub fn get_pos_table(&self) -> Vec<Position> {
        let res = (*self.internal.lock().unwrap()).pos_table.clone();
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
    total_line_number_approx: Option<usize>,
    pos_table: Vec<Position>,
    done: bool,
}

impl ReaderInternalState {
    fn init_internal(config: Arc<CsvConfig>) -> (Arc<Mutex<ReaderInternalState>>, JoinHandle<()>) {
        let internal = ReaderInternalState {
            total_line_number: None,
            total_line_number_approx: None,
            pos_table: vec![],
            done: false,
        };

        let m_state = Arc::new(Mutex::new(internal));

        let _m = m_state.clone();
        let handle = thread::spawn(move || {
            // quick line count
            let total_line_number_approx;
            {
                let file = File::open(config.filename()).unwrap();
                let buf_reader = BufReader::new(file);
                // subtract 1 for headers
                total_line_number_approx = buf_reader.lines().count().saturating_sub(1);

                let mut m = _m.lock().unwrap();
                (*m).total_line_number_approx = Some(total_line_number_approx);
            }

            let pos_table_num_entries = 10000;
            let minimum_interval = 100; // handle small csv (don't keep pos every line)
            let pos_table_update_every = max(
                minimum_interval,
                total_line_number_approx / pos_table_num_entries,
            );

            // full csv parsing
            let bg_reader = config.new_reader().unwrap();
            let mut n = 0;
            let mut iter = bg_reader.into_records();
            loop {
                let next_pos = iter.reader().position().clone();
                if iter.next().is_none() {
                    break;
                }
                // must not include headers position here (n > 0)
                if n > 0 && n % pos_table_update_every == 0 {
                    let mut m = _m.lock().unwrap();
                    (*m).pos_table.push(next_pos);
                }
                n += 1;
            }
            let mut m = _m.lock().unwrap();
            (*m).total_line_number = Some(n);
            (*m).done = true;
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
        let config = Arc::new(CsvConfig::new("tests/data/cities.csv"));
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
        let config = Arc::new(CsvConfig::new("tests/data/simple.csv"));
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
        let config = Arc::new(CsvConfig::new("tests/data/simple.csv"));
        let mut r = CsvLensReader::new(config).unwrap();
        r.wait_internal();
        let indices = vec![5000];
        let (rows, _stats) = r.get_rows_impl(&indices).unwrap();
        assert_eq!(rows, vec![]);
    }

    #[test]
    fn test_simple_get_rows_impl_1() {
        let config = Arc::new(CsvConfig::new("tests/data/simple.csv"));
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
            num_seek: 49,
            num_parsed_record: 505,
        };
        assert_eq!(stats, expected);
    }

    #[test]
    fn test_simple_get_rows_impl_2() {
        let config = Arc::new(CsvConfig::new("tests/data/simple.csv"));
        let mut r = CsvLensReader::new(config).unwrap();
        r.wait_internal();
        let indices = vec![1234];
        let (rows, stats) = r.get_rows_impl(&indices).unwrap();
        let expected = vec![Row::new(1235, vec!["A1235", "B1235"])];
        assert_eq!(rows, expected);
        let expected = GetRowsStats {
            num_seek: 12,
            num_parsed_record: 35,
        };
        assert_eq!(stats, expected);
    }

    #[test]
    fn test_simple_get_rows_impl_3() {
        let config = Arc::new(CsvConfig::new("tests/data/simple.csv"));
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
        let config = Arc::new(CsvConfig::new("tests/data/small.csv"));
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
        let mut config = CsvConfig::new("tests/data/small.bsv");
        config.delimiter = b'|';
        let config = Arc::new(config);
        let mut r = CsvLensReader::new(config).unwrap();
        let rows = r.get_rows(0, 50).unwrap();
        let expected = vec![
            Row::new(1, vec!["c1", " v1"]),
            Row::new(2, vec!["c2", " v2"]),
        ];
        assert_eq!(rows, expected);
    }

    #[test]
    fn test_irregular() {
        let config = Arc::new(CsvConfig::new("tests/data/irregular.csv"));
        let mut r = CsvLensReader::new(config).unwrap();
        let rows = r.get_rows(0, 50).unwrap();
        let expected = vec![Row::new(1, vec!["c1"]), Row::new(2, vec!["c2", " v2"])];
        assert_eq!(rows, expected);
    }
}
