extern crate csv;

use csv::{Position, Reader};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

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

impl CsvLensReader {

    pub fn new(filename: &str) -> Self {

        let mut reader = Reader::from_path(filename).unwrap();
        let headers_record = reader.headers().unwrap();
        let headers = string_record_to_vec(headers_record);

        let (m_internal, handle) = ReaderInternalState::init_internal(filename);

        Self {
            reader: reader,
            headers: headers,
            internal: m_internal,
            bg_handle: handle,
        }
    }

    pub fn get_rows(&mut self, rows_from: u64, num_rows: u64) -> csv::Result<Vec<Vec<String>>> {

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

        let records = self.reader.records();
        let mut res = Vec::new();

        let mut records_iter = records.into_iter();
        loop {
            let next_record_index = records_iter.reader().position().record();
            if let Some(r) = records_iter.next() {
                // no effective pre-seeking happened, this is still the header
                if next_record_index == 0 {
                    continue;
                }
                // rows_from is 0-based
                if next_record_index - 1 >= rows_from {
                    let string_record = r.unwrap();
                    let mut row = Vec::new();
                    for field in string_record.iter() {
                        row.push(String::from(field));
                    }
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
}

struct ReaderInternalState {
    total_line_number: Option<usize>,
    total_line_number_approx: Option<usize>,
    pos_table: Vec<Position>,
}

impl ReaderInternalState {

    fn init_internal(filename: &str) -> (Arc<Mutex<ReaderInternalState>>, JoinHandle<()>) {

        let internal = ReaderInternalState {
            total_line_number: None,
            total_line_number_approx: None,
            pos_table: vec![],
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
            let pos_table_update_every = total_line_number_approx / pos_table_num_entries;

            // full csv parsing
            let bg_reader = Reader::from_path(_filename.as_str()).unwrap();
            let mut n = 0;
            let mut iter = bg_reader.into_records();
            loop {
                let next_pos = iter.reader().position().clone();
                if let None = iter.next() {
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
        });

        (m_state, handle)
    }

}