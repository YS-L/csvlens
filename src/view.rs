use crate::csv::CsvLensReader;
use crate::input::Control;

use std::time::Instant;

pub struct RowsView {
    reader: CsvLensReader,
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
    num_rows: u64,
    rows_from: u64,
    elapsed: Option<u128>,
}

impl RowsView {

   pub fn new(mut reader: CsvLensReader, num_rows: u64) -> RowsView {
       let rows_from = 0;
       let rows = reader.get_rows(rows_from, num_rows).unwrap();
       let headers = reader.headers.clone();
       Self {
           reader,
           headers,
           rows,
           num_rows,
           rows_from,
           elapsed: None,
       }
   }

   pub fn headers(&self) -> &Vec<String> {
       &self.headers
   }

   pub fn rows(&self) -> &Vec<Vec<String>> {
       &self.rows
   }

   pub fn num_rows(&self) -> u64 {
       self.num_rows
   }

   pub fn set_num_rows(&mut self, num_rows: u64) {
       if num_rows == self.num_rows {
           return;
       }
       self.num_rows = num_rows;
       self.do_get_rows();
   }

   pub fn rows_from(&self) -> u64 {
       self.rows_from
   }

   pub fn set_rows_from(&mut self, rows_from: u64) {
       if rows_from == self.rows_from {
           return;
       }
       self.rows_from = rows_from;
       self.do_get_rows();
   }

   pub fn elapsed(&self) -> Option<u128> {
        self.elapsed
   }

   pub fn get_total_line_numbers(&self) -> Option<usize> {
       self.reader.get_total_line_numbers()
   }

   pub fn get_total_line_numbers_approx(&self) -> Option<usize> {
       self.reader.get_total_line_numbers_approx()
   }

   pub fn handle_control(&mut self, control: &Control) {
       match control {
           Control::ScrollDown => {
               self.increase_rows_from(1);
           }
           Control::ScrollPageDown => {
               self.increase_rows_from(self.num_rows);
           }
           Control::ScrollUp => {
               self.decrease_rows_from(1);
           }
           Control::ScrollPageUp => {
               self.decrease_rows_from(self.num_rows);
           }
           Control::ScrollBottom => {
                if let Some(total) =
                    self.reader
                        .get_total_line_numbers()
                        .or(self.reader.get_total_line_numbers_approx()) {
                    // TODO: fix type conversion craziness
                    let rows_from = total.saturating_sub(self.num_rows as usize) as u64;
                    self.set_rows_from(rows_from);
                }
           }
           Control::ScrollTo(n) => {
                // TODO: handle value larger than total number of records
                let rows_from = n.saturating_sub(1) as u64;
                self.set_rows_from(rows_from);
           }
           _ => {}
       }
   }

   fn increase_rows_from(&mut self, delta: u64) {
       let mut new_rows_from = self.rows_from.saturating_add(delta);
       if let Some(n) = self.bottom_rows_from() {
           if new_rows_from >= n {
               new_rows_from = n;
           }
       }
       self.set_rows_from(new_rows_from);
   }

   fn bottom_rows_from(&self) -> Option<u64> {
       // fix type conversion craziness
       if let Some(n) = self.reader.get_total_line_numbers() {
           return Some(n.saturating_sub(self.num_rows as usize) as u64);
       }
       None
   }

   fn decrease_rows_from(&mut self, delta: u64) {
       let new_rows_from = self.rows_from.saturating_sub(delta);
       self.set_rows_from(new_rows_from);
   }

   fn do_get_rows(&mut self) {
       let start = Instant::now();
       let rows = self.reader.get_rows(self.rows_from, self.num_rows).unwrap();
       let elapsed = start.elapsed().as_micros();
       self.rows = rows;
       self.elapsed = Some(elapsed);
   }

}