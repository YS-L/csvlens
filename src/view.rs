use crate::csv::{CsvLensReader, Row};
use crate::input::Control;

use anyhow::Result;
use std::time::Instant;
use std::cmp::min;

pub struct RowsView {
    reader: CsvLensReader,
    headers: Vec<String>,
    rows: Vec<Row>,
    num_rows: u64,
    rows_from: u64,
    filter_indices: Option<Vec<u64>>,
    elapsed: Option<u128>,
}

impl RowsView {

   pub fn new(mut reader: CsvLensReader, num_rows: u64) -> Result<RowsView> {
       let rows_from = 0;
       let rows = reader.get_rows(rows_from, num_rows)?;
       let headers = reader.headers.clone();
       let view = Self {
           reader,
           headers,
           rows,
           num_rows,
           rows_from,
           filter_indices: None,
           elapsed: None,
       };
       Ok(view)
   }

   pub fn headers(&self) -> &Vec<String> {
       &self.headers
   }

   pub fn rows(&self) -> &Vec<Row> {
       &self.rows
   }

   pub fn num_rows(&self) -> u64 {
       self.num_rows
   }

   pub fn set_num_rows(&mut self, num_rows: u64) -> Result<()> {
       if num_rows == self.num_rows {
           return Ok(());
       }
       self.num_rows = num_rows;
       self.do_get_rows()?;
       Ok(())
   }

   pub fn set_filter(&mut self, filter_indices: &[u64]) -> Result<()> {
       if let Some(indices) = &self.filter_indices {
           if indices == filter_indices {
               return Ok(());
           }
       }
       self.filter_indices = Some(filter_indices.to_vec());
       self.do_get_rows()
   }

   pub fn is_filter(&self) -> bool {
       self.filter_indices.is_some()
   }

   pub fn reset_filter(&mut self) -> Result<()> {
       if self.filter_indices.is_none() {
           return Ok(());
       }
       self.filter_indices = None;
       self.do_get_rows()
   }

   pub fn init_filter(&mut self) -> Result<()> {
       self.set_filter(&vec![])
   }

   pub fn rows_from(&self) -> u64 {
       self.rows_from
   }

   pub fn set_rows_from(&mut self, rows_from_: u64) -> Result<()> {
       let rows_from;
       if let Some(n) = self.bottom_rows_from() {
           rows_from = min(rows_from_, n);
       }
       else {
           rows_from = rows_from_;
       }
       if rows_from == self.rows_from {
           return Ok(());
       }
       self.rows_from = rows_from;
       self.do_get_rows()?;
       Ok(())
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

   pub fn in_view(&self, row_index: u64) -> bool {
       let last_row = self.rows_from().saturating_add(self.num_rows());
       if row_index >= self.rows_from() && row_index < last_row {
           return true;
       }
       false
   }

   pub fn handle_control(&mut self, control: &Control) -> Result<()> {
       match control {
           Control::ScrollDown => {
               self.increase_rows_from(1)?;
           }
           Control::ScrollPageDown => {
               self.increase_rows_from(self.num_rows)?;
           }
           Control::ScrollUp => {
               self.decrease_rows_from(1)?;
           }
           Control::ScrollPageUp => {
               self.decrease_rows_from(self.num_rows)?;
           }
           Control::ScrollBottom => {
               if let Some(total) = self.get_total() {
                    let rows_from = total.saturating_sub(self.num_rows as usize) as u64;
                    self.set_rows_from(rows_from)?;
               }
           }
           Control::ScrollTo(n) => {
                let mut rows_from = n.saturating_sub(1) as u64;
                if let Some(n) = self.bottom_rows_from() {
                    rows_from = min(rows_from, n);
                }
                self.set_rows_from(rows_from)?;
           }
           _ => {}
       }
       Ok(())
   }

   fn get_total(&self) -> Option<usize> {
       if let Some(indices) = &self.filter_indices {
           return Some(indices.len());
       }
       else {
           if let Some(n) = self.reader.get_total_line_numbers().or_else(
               || self.reader.get_total_line_numbers_approx()
           ) {
               return Some(n);
           }
       }
       None
   }

   fn increase_rows_from(&mut self, delta: u64) -> Result<()> {
       let new_rows_from = self.rows_from.saturating_add(delta);
       self.set_rows_from(new_rows_from)?;
       Ok(())
   }

   fn decrease_rows_from(&mut self, delta: u64) -> Result<()> {
       let new_rows_from = self.rows_from.saturating_sub(delta);
       self.set_rows_from(new_rows_from)?;
       Ok(())
   }

   fn bottom_rows_from(&self) -> Option<u64> {
       // fix type conversion craziness
       if let Some(n) = self.get_total() {
           return Some(n.saturating_sub(self.num_rows as usize) as u64);
       }
       None
   }

    fn do_get_rows(&mut self) -> Result<()> {
        let start = Instant::now();
        let rows;
        if let Some(indices) = &self.filter_indices {
            let start = self.rows_from as usize;
            let start = min(start, indices.len().saturating_sub(1));
            let end = start.saturating_add(self.num_rows as usize);
            let end = min(end, indices.len());
            let indices = &indices[start..end];
            rows = self.reader.get_rows_for_indices(indices)?;
        }
        else {
            rows = self.reader.get_rows(self.rows_from, self.num_rows)?;
        }
        let elapsed = start.elapsed().as_micros();
        self.rows = rows;
        self.elapsed = Some(elapsed);
        Ok(())
    }

}