extern crate csv;

use anyhow::Result;
use csv::Reader;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

pub struct Finder {
    internal: Arc<Mutex<FinderInternalState>>,
    cursor: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct FoundRecord {
    row_index: usize,
    column_indices: Vec<usize>,
}

impl FoundRecord {
    pub fn row_index(&self) -> usize {
        self.row_index
    }

    pub fn column_indices(&self) -> &Vec<usize> {
        &self.column_indices
    }

    pub fn first_column(&self) -> usize {
        *self.column_indices.first().unwrap()
    }
}

impl Finder {

    pub fn new(filename: &str, target: &str) -> Result<Self> {
        let internal = FinderInternalState::init(
            filename, target
        );
        let finder = Finder {
            internal,
            cursor: None,
        };
        Ok(finder)
    }

    pub fn count(&self) -> usize {
        (self.internal.lock().unwrap()).count
    }

    pub fn done(&self) -> bool {
        (self.internal.lock().unwrap()).done
    }

    pub fn cursor(&self) -> Option<usize> {
        self.cursor
    }

    pub fn next(&mut self) -> Option<FoundRecord> {
        let count = self.count();
        if let Some(n) = self.cursor {
            if n + 1 < count {
                self.cursor = Some(n + 1);
            }
        }
        else {
            if count > 0 {
                self.cursor = Some(0);
            }
        }
        self.get_found_record_at_cursor()
    }

    pub fn prev(&mut self) -> Option<FoundRecord> {
        if let Some(n) = self.cursor {
            self.cursor = Some(n.saturating_sub(1));
        }
        else {
            let count = self.count();
            if count > 0 {
                self.cursor = Some(0);
            }
        }
        self.get_found_record_at_cursor()
    }

    fn get_found_record_at_cursor(&self) -> Option<FoundRecord> {
        if let Some(n) = self.cursor {
            let m_guard= self.internal.lock().unwrap();
            // TODO: this weird ref massaging really needed?
            let res = m_guard.founds.get(n);
            if let Some(r) = res {
                Some(r.clone())
            }
            else {
                None
            }
        }
        else {
            None
        }
    }    

    pub fn get_all_found(&self) -> Vec<FoundRecord> {
        let m_guard = self.internal.lock().unwrap();
        m_guard.founds.clone()
    }
}

struct FinderInternalState {
    target: String,
    count: usize,
    founds: Vec<FoundRecord>,
    done: bool,
}

impl FinderInternalState {

    pub fn init(filename: &str, target: &str) -> Arc<Mutex<FinderInternalState>> {

        let internal = FinderInternalState {
            target: target.to_owned(),
            count: 0,
            founds: vec![],
            done: false,
        };

        let m_state = Arc::new(Mutex::new(internal));

        let _m = m_state.clone();
        let _filename = filename.to_owned();
        let _target = target.to_owned();

        let handle = thread::spawn(move|| {
            
            let mut bg_reader = Reader::from_path(_filename.as_str()).unwrap();

            // note that records() exludes header
            let records = bg_reader.records();

            for (row_index, r) in records.enumerate() {
                let mut column_indices = vec![];
                if let Ok(valid_record) = r {
                    for (column_index, field) in valid_record.iter().enumerate() {
                        if field.contains(_target.as_str()) {
                            column_indices.push(column_index);
                        }
                    }
                }
                if !column_indices.is_empty() {
                    let found = FoundRecord {
                        row_index,
                        column_indices,
                    };
                    let mut m = _m.lock().unwrap();
                    (*m).found_one(found);
                }
            }

            let mut m = _m.lock().unwrap();
            (*m).done = true;
            
        });

        m_state
    }

    fn found_one(&mut self, found: FoundRecord) {
        self.founds.push(found);
        self.count += 1;
    }
}