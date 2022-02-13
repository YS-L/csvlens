extern crate csv;

use anyhow::Result;
use csv::{Position, Reader};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time;
use std::cmp::max;


fn string_record_to_vec(record: &csv::StringRecord) -> Vec<String> {
    let mut string_vec= Vec::new();
    for field in record.iter() {
        string_vec.push(String::from(field));
    }
    string_vec
}

pub struct CsvLensReader {
    reader: Reader<File>,
    pub headers: Vec<String>,
    internal: Arc<Mutex<ReaderInternalState>>,
    bg_handle: thread::JoinHandle<()>,
}

#[derive(Debug, PartialEq)]
pub struct Row {
    pub record_num: usize,
    pub fields: Vec<String>,
}

impl Row {
    pub fn new(record_num: usize, fields: Vec<&str>) -> Row {
        Row {
            record_num,
            fields: fields.iter().map(|x| x.to_string()).collect()
        }
    }
}

impl CsvLensReader {

    pub fn new(filename: &str) -> Result<Self> {

        let mut reader = Reader::from_path(filename)?;
        let headers_record = reader.headers().unwrap();
        let headers = string_record_to_vec(headers_record);

        let (m_internal, handle) = ReaderInternalState::init_internal(filename);

        let reader = Self {
            reader,
            headers,
            internal: m_internal,
            bg_handle: handle,
        };
        Ok(reader)
    }

    pub fn get_rows(&mut self, rows_from: u64, num_rows: u64) -> Result<Vec<Row>> {

        // seek to the closest previously known position
        let mut pos = Position::new();
        let pos_table = self.get_pos_table();
        for p in pos_table.into_iter() {
            // can safely -1 because first position in the table must not be for headers
            if p.record() - 1 <= rows_from {
                pos = p;
            }
        }
        self.reader.seek(pos)?;

        // note that records() excludes header by default, but here the first
        // entry is header because of the seek() above.
        let mut records = self.reader.records();
        let mut res = Vec::new();

        loop {
            let next_record_index = records.reader().position().record();
            if let Some(r) = records.next() {
                // no effective pre-seeking happened, this is still the header
                if next_record_index == 0 {
                    continue;
                }
                // rows_from is 0-based
                if next_record_index - 1 >= rows_from {
                    let string_record = r?;
                    let mut fields= Vec::new();
                    for field in string_record.iter() {
                        fields.push(String::from(field));
                    }
                    let row = Row {
                        record_num: next_record_index as usize,
                        fields,
                    };
                    res.push(row);
                }
                if res.len() >= num_rows as usize {
                    break;
                }
            }
            else {
                break;
            }
        }

        Ok(res)
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

    fn wait_internal(&self) {
        loop {
            if self.internal.lock().unwrap().done {
                break
            }
            thread::sleep(time::Duration::from_millis(100));
        }
    }
}

struct ReaderInternalState {
    total_line_number: Option<usize>,
    total_line_number_approx: Option<usize>,
    pos_table: Vec<Position>,
    done: bool,
}

impl ReaderInternalState {

    fn init_internal(filename: &str) -> (Arc<Mutex<ReaderInternalState>>, JoinHandle<()>) {

        let internal = ReaderInternalState {
            total_line_number: None,
            total_line_number_approx: None,
            pos_table: vec![],
            done: false,
        };

        let m_state = Arc::new(Mutex::new(internal));

        let _m = m_state.clone();
        let _filename = filename.to_string();
        let handle = thread::spawn(move || {

            // quick line count
            let total_line_number_approx;
            {
                let file = File::open(_filename.as_str()).unwrap();
                let buf_reader = BufReader::new(file);
                // subtract 1 for headers
                total_line_number_approx = buf_reader.lines().count().saturating_sub(1);

                let mut m= _m.lock().unwrap();
                (*m).total_line_number_approx = Some(total_line_number_approx);
            }

            let pos_table_num_entries = 1000;
            let minimum_interval = 100;  // handle small csv (don't keep pos every line)
            let pos_table_update_every = max(
                minimum_interval, total_line_number_approx / pos_table_num_entries
            );

            // full csv parsing
            let bg_reader = Reader::from_path(_filename.as_str()).unwrap();
            let mut n = 0;
            let mut iter = bg_reader.into_records();
            loop {
                let next_pos = iter.reader().position().clone();
                if iter.next().is_none() {
                    break;
                }
                // must not include headers position here (n > 0)
                if n > 0 && n % pos_table_update_every == 0 {
                    let mut m= _m.lock().unwrap();
                    (*m).pos_table.push(next_pos);
                }
                n += 1;
            }
            let mut m= _m.lock().unwrap();
            (*m).total_line_number = Some(n);
            (*m).done = true;
        });

        (m_state, handle)
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cities_get_rows() {
        let mut r = CsvLensReader::new("tests/data/cities.csv").unwrap();
        r.wait_internal();
        let rows = r.get_rows(2, 3).unwrap();
        let expected = vec![
            Row::new(3, vec!["46", "35", "59", "N", "120", "30", "36", "W", "Yakima", "WA"]),
            Row::new(4, vec!["42", "16", "12", "N", "71", "48", "0", "W", "Worcester", "MA"]),
            Row::new(5, vec!["43", "37", "48", "N", "89", "46", "11", "W", "Wisconsin Dells", "WI"]),
        ];
        assert_eq!(rows, expected);
    }

    #[test]
    fn test_simple_get_rows() {
        let mut r = CsvLensReader::new("tests/data/simple.csv").unwrap();
        r.wait_internal();
        let rows = r.get_rows(1234, 2).unwrap();
        let expected = vec![
            Row::new(1235, vec!["A1235", "B1235"]),
            Row::new(1236, vec!["A1236", "B1236"]),
        ];
        assert_eq!(rows, expected);
    }
}