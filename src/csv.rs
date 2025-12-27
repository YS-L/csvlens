extern crate csv;

use csv::{Position, Reader, ReaderBuilder};
use std::cmp::max;
use std::fs::File;
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time;
use std::{
    io::{self, Read},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use csv::{ByteRecord, StringRecord};
use csv_core::Reader as CoreReader;
use csv_core::ReaderBuilder as CoreReaderBuilder;

use crate::errors::CsvlensResult;

fn string_record_to_vec(record: &csv::StringRecord) -> Vec<String> {
    let mut string_vec = Vec::with_capacity(record.len());
    for field in record.iter() {
        string_vec.push(String::from(field));
    }
    string_vec
}

pub struct CsvBaseConfig {
    delimiter: u8,
    no_headers: bool,
}

impl CsvBaseConfig {
    pub fn new(delimiter: u8, no_headers: bool) -> CsvBaseConfig {
        CsvBaseConfig {
            delimiter,
            no_headers,
        }
    }
}

pub struct CsvConfig {
    path: String,
    stream_active: Option<Arc<AtomicBool>>,
    base: CsvBaseConfig,
}

impl CsvConfig {
    pub fn new(
        path: &str,
        stream_active: Option<Arc<AtomicBool>>,
        base: CsvBaseConfig,
    ) -> CsvConfig {
        CsvConfig {
            path: path.to_string(),
            stream_active,
            base,
        }
    }

    pub fn new_reader(&self) -> CsvlensResult<Reader<File>> {
        let reader = ReaderBuilder::new()
            .flexible(true)
            .delimiter(self.base.delimiter)
            .has_headers(!self.base.no_headers)
            .from_path(self.path.as_str())?;
        Ok(reader)
    }

    pub fn new_core_reader(&self) -> CoreReader {
        CoreReaderBuilder::new()
            .delimiter(self.base.delimiter)
            .build()
    }

    pub fn filename(&self) -> &str {
        self.path.as_str()
    }

    pub fn delimiter(&self) -> u8 {
        self.base.delimiter
    }

    pub fn no_headers(&self) -> bool {
        self.base.no_headers
    }

    pub fn has_headers(&self) -> bool {
        !self.base.no_headers
    }

    /// Convert position to a 0-based record index
    pub fn position_to_record_index(&self, position: u64) -> u64 {
        if self.base.no_headers {
            position
        } else {
            position - 1
        }
    }

    /// Convert position to a 1-based record number
    pub fn position_to_record_num(&self, position: u64) -> u64 {
        if self.base.no_headers {
            position + 1
        } else {
            position
        }
    }

    /// Whether the file should be read in streaming mode, and whether the stream is still active
    pub fn is_streaming(&self) -> bool {
        self.stream_active
            .as_ref()
            .map(|x| x.load(Ordering::Relaxed))
            .unwrap_or(false)
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
            if let Some(field) = self.fields.get(*i) {
                subfields.push(field.clone());
            }
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

impl Drop for CsvLensReader {
    fn drop(&mut self) {
        self.terminate();
    }
}

impl CsvLensReader {
    pub fn new(config: Arc<CsvConfig>) -> CsvlensResult<Self> {
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

        // _handle.join().unwrap();

        let reader = Self {
            config: config.clone(),
            reader,
            headers,
            internal: m_internal,
        };
        Ok(reader)
    }

    pub fn get_rows(
        &mut self,
        rows_from: u64,
        num_rows: u64,
    ) -> CsvlensResult<(Vec<Row>, GetRowsStats)> {
        let indices: Vec<u64> = (rows_from..rows_from + num_rows).collect();
        self.get_rows_impl(&indices)
    }

    pub fn get_rows_for_indices(
        &mut self,
        indices: &[u64],
    ) -> CsvlensResult<(Vec<Row>, GetRowsStats)> {
        self.get_rows_impl(indices)
    }

    fn get_rows_impl(&mut self, indices: &[u64]) -> CsvlensResult<(Vec<Row>, GetRowsStats)> {
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
    ) -> CsvlensResult<(Vec<Row>, GetRowsStats)> {
        // stats for debugging and testing
        let mut stats = GetRowsStats::new();

        let pos = Position::new();
        self.reader.seek(pos)?;

        let tic = time::Instant::now();
        let pos_table = self.get_pos_table();
        stats.pos_table_elapsed = Some(tic.elapsed());
        stats.pos_table_entry = pos_table.len();

        let mut pos_iter = pos_table.iter();
        let mut indices_iter = indices.iter();

        let mut res = vec![Row::empty(); indices.len()];
        let mut res_max_index: Option<usize> = None;

        let mut next_pos = pos_iter.next();
        let mut next_wanted = indices_iter.next();

        let num_fields = self.headers.len();

        let mut should_stop = false;
        loop {
            if next_wanted.is_none() {
                break;
            }
            // seek as close to the next wanted record index as possible
            let index = next_wanted.unwrap();
            let mut seek_pos: Option<Position> = None;
            while let Some(pos) = next_pos {
                if self.config.position_to_record_index(pos.record()) <= index.record_index {
                    seek_pos.replace(pos.clone());
                } else {
                    break;
                }
                next_pos = pos_iter.next();
            }
            if let Some(pos) = seek_pos {
                self.reader.seek(pos.clone())?;
                stats.log_seek();
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
                        let mut fields = Vec::with_capacity(num_fields);
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
                    if let Some(pos) = next_pos
                        && record_position >= pos.record()
                    {
                        break;
                    }
                } else {
                    // no more records
                    should_stop = true;
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

            if should_stop {
                // no more records, no point continuing even if there are more marked positions.
                // This could be caused by out of bound indices or changed file content.
                break;
            }
        }

        // In case requested indices are beyond the last record, truncate those indices.
        res.truncate(res_max_index.map_or(0, |x| x + 1));

        Ok((res, stats))
    }

    pub fn get_approx_line_numbers(&self) -> usize {
        self.internal
            .lock()
            .unwrap()
            .current_line_number
            .load(Ordering::Relaxed)
    }

    pub fn get_total_line_numbers(&self) -> Option<usize> {
        self.internal.lock().unwrap().total_line_number
    }

    pub fn get_pos_table(&self) -> Vec<Position> {
        self.internal.lock().unwrap().pos_table.clone()
    }

    fn terminate(&self) {
        let mut m_guard = self.internal.lock().unwrap();
        m_guard.terminate();
    }

    #[cfg(test)]
    fn wait_till_start_scanning(&self) {
        loop {
            if self.internal.lock().unwrap().started_scanning {
                break;
            }
            thread::sleep(time::Duration::from_millis(100));
        }
    }

    #[cfg(test)]
    pub fn wait_internal(&self) {
        loop {
            if self.internal.lock().unwrap().done {
                break;
            }
            thread::sleep(time::Duration::from_millis(100));
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GetRowsStats {
    pub num_seek: u64,
    pub num_parsed_record: u64,
    pub pos_table_elapsed: Option<time::Duration>,
    pub pos_table_entry: usize,
}

impl GetRowsStats {
    fn new() -> GetRowsStats {
        GetRowsStats {
            num_seek: 0,
            num_parsed_record: 0,
            pos_table_elapsed: None,
            pos_table_entry: 0,
        }
    }

    fn log_seek(&mut self) {
        self.num_seek += 1;
    }

    fn log_parsed_record(&mut self) {
        self.num_parsed_record += 1
    }
}

pub struct StreamingCsvReader {
    file: File,
    core: CoreReader,
    in_buf: Vec<u8>,
    buf_start: usize,
    buf_end: usize,
    fields: Vec<u8>,
    ends: Vec<usize>,
    cur_pos: Position,
    first_record_returned: bool,
    config: Arc<CsvConfig>,
    sleep: Duration,
}

impl StreamingCsvReader {
    pub fn new(csv_config: Arc<CsvConfig>) -> io::Result<Self> {
        let file = File::open(csv_config.path.as_str())?;
        let core = csv_config.new_core_reader();
        // TODO: these initial capacities ok?
        Ok(Self {
            file,
            core,
            in_buf: vec![0u8; 64 * 1024],
            buf_start: 0,
            buf_end: 0,
            fields: vec![0u8; 8 * 1024],
            ends: vec![0; 256],
            cur_pos: Position::new(),
            first_record_returned: false,
            config: csv_config,
            sleep: Duration::from_millis(200),
        })
    }

    fn read_buffer(&mut self) -> io::Result<()> {
        self.buf_start = 0;
        let n = self.file.read(&mut self.in_buf)?;
        self.buf_end = n;
        Ok(())
    }

    fn build_byte_record(&self, fields: &[u8], ends: &[usize], pos: Position) -> ByteRecord {
        let mut rec = ByteRecord::new();
        let mut start = 0usize;
        for &end in ends {
            let field_bytes = &fields[start..end];
            rec.push_field(field_bytes);
            start = end;
        }
        rec.set_position(Some(pos.clone()));
        rec
    }

    #[inline(always)]
    fn read_string_record(&mut self) -> Option<CsvlensResult<StringRecord>> {
        use csv_core::ReadRecordResult::*;

        let (mut outlen, mut endlen) = (0, 0);
        let record_pos = self.cur_pos.clone();
        loop {
            // If no input left in buffer, try to read more
            if self.buf_start == self.buf_end {
                if let Err(e) = self.read_buffer() {
                    return Some(Err(e.into()));
                }

                if self.buf_end == 0 {
                    if !self.config.is_streaming() {
                        break;
                    }
                    // Temporary EOF: no new bytes right now. In streaming mode we just wait and
                    // try again.
                    thread::sleep(self.sleep);
                    continue;
                }
            }

            // Similar implementation as csv crate's read_byte_record_impl but blocks on EOF to
            // allow tailing
            let (res, nin, nout, nend) = {
                let input = &self.in_buf[self.buf_start..self.buf_end];
                self.core
                    .read_record(input, &mut self.fields[outlen..], &mut self.ends[endlen..])
            };
            let byte = self.cur_pos.byte();
            self.cur_pos
                .set_byte(byte + nin as u64)
                .set_line(self.core.line());
            self.buf_start += nin;
            outlen += nout;
            endlen += nend;
            match res {
                InputEmpty => continue,
                OutputFull => {
                    let new_len = self.fields.len() * 2;
                    self.fields.resize(new_len, 0);
                    continue;
                }
                OutputEndsFull => {
                    let new_len = self.ends.len() * 2;
                    self.ends.resize(new_len, 0);
                    continue;
                }
                Record => {
                    let byte_rec = self.build_byte_record(
                        &self.fields[..outlen],
                        &self.ends[..endlen],
                        record_pos,
                    );
                    self.cur_pos
                        .set_record(self.cur_pos.record().checked_add(1).unwrap());
                    match StringRecord::from_byte_record(byte_rec) {
                        Ok(srec) => return Some(Ok(srec)),
                        Err(e) => return Some(Err(e.into())),
                    }
                }
                End => {}
            }
        }

        // Handle any remaining partial record at true EOF
        if endlen > 0 {
            let byte_rec =
                self.build_byte_record(&self.fields[..outlen], &self.ends[..endlen], record_pos);
            match StringRecord::from_byte_record(byte_rec) {
                Ok(srec) => return Some(Ok(srec)),
                Err(e) => return Some(Err(e.into())),
            }
        }

        None
    }

    fn reader_position(&self) -> &Position {
        &self.cur_pos
    }
}

impl Iterator for StreamingCsvReader {
    type Item = CsvlensResult<StringRecord>;

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        // For the first record, if there are headers, skip it
        let mut record = self.read_string_record();
        if self.config.has_headers() && !self.first_record_returned {
            record = self.read_string_record();
        }
        self.first_record_returned = true;
        record
    }
}

pub enum CsvlensRecordIterator {
    Streaming(Box<StreamingCsvReader>),
    Standard(csv::StringRecordsIntoIter<File>),
}

impl CsvlensRecordIterator {
    pub fn new(config: Arc<CsvConfig>) -> CsvlensResult<CsvlensRecordIterator> {
        Ok(if config.is_streaming() {
            CsvlensRecordIterator::Streaming(Box::new(StreamingCsvReader::new(config)?))
        } else {
            let reader = config.new_reader()?;
            CsvlensRecordIterator::Standard(reader.into_records())
        })
    }

    pub fn position(&self) -> &Position {
        match self {
            CsvlensRecordIterator::Streaming(iter) => iter.reader_position(),
            CsvlensRecordIterator::Standard(iter) => iter.reader().position(),
        }
    }
}

impl Iterator for CsvlensRecordIterator {
    type Item = CsvlensResult<csv::StringRecord>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            CsvlensRecordIterator::Streaming(iter) => iter.next(),
            CsvlensRecordIterator::Standard(iter) => iter.next().map(|item| match item {
                Ok(record) => Ok(record),
                Err(e) => Err(e.into()),
            }),
        }
    }
}

struct ReaderInternalState {
    total_line_number: Option<usize>,
    current_line_number: Arc<AtomicUsize>,
    pos_table: Vec<Position>,
    done: bool,
    should_terminate: bool,
    #[cfg(test)]
    started_scanning: bool,
}

impl ReaderInternalState {
    fn init_internal(config: Arc<CsvConfig>) -> (Arc<Mutex<ReaderInternalState>>, JoinHandle<()>) {
        // current_line_number will be updated every record, so need a lock free way to update it
        let current_line_number = Arc::new(AtomicUsize::new(0));

        let internal = ReaderInternalState {
            total_line_number: None,
            current_line_number: current_line_number.clone(),
            pos_table: vec![],
            done: false,
            should_terminate: false,
            #[cfg(test)]
            started_scanning: false,
        };

        let m_state = Arc::new(Mutex::new(internal));

        let _m = m_state.clone();
        let handle = thread::spawn(move || {
            let pos_table_update_every = if config.is_streaming() {
                // When streaming, filesize cannot be determined. Use a larger default of 64KB (16K
                // entries for 1GB file, pos table size: 384 KB)
                #[cfg(test)]
                {
                    500
                }
                #[cfg(not(test))]
                {
                    64 * 1024
                }
            } else {
                let filesize = File::open(config.filename())
                    .unwrap()
                    .metadata()
                    .unwrap()
                    .len();
                let pos_table_num_entries = 10000;
                let minimum_interval = 500; // handle small csv (don't keep pos every byte)
                max(minimum_interval, filesize / pos_table_num_entries)
            };

            // full csv parsing
            let mut n_lines = 0;
            let mut n_bytes: u64 = 0;
            let mut last_updated_at = 0;
            let mut iter = CsvlensRecordIterator::new(config).unwrap();

            #[cfg(test)]
            {
                _m.lock().unwrap().started_scanning = true;
            }

            loop {
                let next_pos = iter.position().clone();
                if iter.next().is_none() {
                    break;
                }
                // must not include headers position here (n > 0)
                let cur = n_bytes / pos_table_update_every;
                if n_bytes > 0 && cur > last_updated_at {
                    let mut m = _m.lock().unwrap();
                    if m.should_terminate {
                        break;
                    }
                    m.pos_table.push(next_pos.clone());
                    last_updated_at = cur;
                }
                n_lines += 1;
                n_bytes = next_pos.byte();
                current_line_number.store(n_lines, Ordering::Relaxed);
            }
            let mut m = _m.lock().unwrap();
            m.total_line_number = Some(n_lines);
            m.done = true;
        });

        (m_state, handle)
    }

    fn terminate(&mut self) {
        self.should_terminate = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    impl Row {
        pub fn new(record_num: usize, fields: Vec<&str>) -> Row {
            Row {
                record_num,
                fields: fields.iter().map(|x| x.to_string()).collect(),
            }
        }
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn test_cities_get_rows(#[case] is_streaming: bool) {
        let stream_active = if is_streaming {
            Some(Arc::new(AtomicBool::new(true)))
        } else {
            None
        };
        let config = Arc::new(CsvConfig::new(
            "tests/data/cities.csv",
            stream_active.clone(),
            CsvBaseConfig::new(b',', false),
        ));
        let mut r = CsvLensReader::new(config).unwrap();
        wait_till_ready(&r, &stream_active);
        let rows = r.get_rows(2, 3).unwrap().0;
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

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn test_simple_get_rows(#[case] is_streaming: bool) {
        let stream_active = if is_streaming {
            Some(Arc::new(AtomicBool::new(true)))
        } else {
            None
        };
        let config = Arc::new(CsvConfig::new(
            "tests/data/simple.csv",
            stream_active.clone(),
            CsvBaseConfig::new(b',', false),
        ));
        let mut r = CsvLensReader::new(config).unwrap();
        wait_till_ready(&r, &stream_active);
        let rows = r.get_rows(1234, 2).unwrap().0;
        let expected = vec![
            Row::new(1235, vec!["A1235", "B1235"]),
            Row::new(1236, vec!["A1236", "B1236"]),
        ];
        assert_eq!(rows, expected);
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn test_simple_get_rows_out_of_bound(#[case] is_streaming: bool) {
        let stream_active = if is_streaming {
            Some(Arc::new(AtomicBool::new(true)))
        } else {
            None
        };
        let config = Arc::new(CsvConfig::new(
            "tests/data/simple.csv",
            stream_active.clone(),
            CsvBaseConfig::new(b',', false),
        ));
        let mut r = CsvLensReader::new(config).unwrap();
        wait_till_ready(&r, &stream_active);
        let indices = vec![5000];
        let (rows, _stats) = r.get_rows_impl(&indices).unwrap();
        assert_eq!(rows, vec![]);
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn test_simple_get_rows_impl_1(#[case] is_streaming: bool) {
        let stream_active = if is_streaming {
            Some(Arc::new(AtomicBool::new(true)))
        } else {
            None
        };
        let config = Arc::new(CsvConfig::new(
            "tests/data/simple.csv",
            stream_active.clone(),
            CsvBaseConfig::new(b',', false),
        ));
        let mut r = CsvLensReader::new(config).unwrap();
        wait_till_ready(&r, &stream_active);
        let indices = vec![1, 3, 5, 1234, 2345, 3456, 4999];
        let (rows, mut stats) = r.get_rows_impl(&indices).unwrap();
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
        stats.pos_table_elapsed.take();
        let expected = GetRowsStats {
            num_seek: 4,
            num_parsed_record: 218,
            pos_table_elapsed: None,
            pos_table_entry: 115,
        };
        assert_eq!(stats, expected);
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn test_simple_get_rows_impl_2(#[case] is_streaming: bool) {
        let stream_active = if is_streaming {
            Some(Arc::new(AtomicBool::new(true)))
        } else {
            None
        };
        let config = Arc::new(CsvConfig::new(
            "tests/data/simple.csv",
            stream_active.clone(),
            CsvBaseConfig::new(b',', false),
        ));
        let mut r = CsvLensReader::new(config).unwrap();
        wait_till_ready(&r, &stream_active);
        let indices = vec![1234];
        let (rows, mut stats) = r.get_rows_impl(&indices).unwrap();
        let expected = vec![Row::new(1235, vec!["A1235", "B1235"])];
        assert_eq!(rows, expected);
        stats.pos_table_elapsed.take();
        let expected = GetRowsStats {
            num_seek: 1,
            num_parsed_record: 8,
            pos_table_elapsed: None,
            pos_table_entry: 115,
        };
        assert_eq!(stats, expected);
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn test_simple_get_rows_impl_3(#[case] is_streaming: bool) {
        let stream_active = if is_streaming {
            Some(Arc::new(AtomicBool::new(true)))
        } else {
            None
        };
        let config = Arc::new(CsvConfig::new(
            "tests/data/simple.csv",
            stream_active.clone(),
            CsvBaseConfig::new(b',', false),
        ));
        let mut r = CsvLensReader::new(config).unwrap();
        wait_till_ready(&r, &stream_active);
        let indices = vec![2];
        let (rows, mut stats) = r.get_rows_impl(&indices).unwrap();
        let expected = vec![Row::new(3, vec!["A3", "B3"])];
        assert_eq!(rows, expected);
        stats.pos_table_elapsed.take();
        let expected = GetRowsStats {
            num_seek: 0,
            num_parsed_record: 4, // 3 + 1 (including header)
            pos_table_elapsed: None,
            pos_table_entry: 115,
        };
        assert_eq!(stats, expected);
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn test_small(#[case] is_streaming: bool) {
        let stream_active = if is_streaming {
            Some(Arc::new(AtomicBool::new(true)))
        } else {
            None
        };
        let config = Arc::new(CsvConfig::new(
            "tests/data/small.csv",
            stream_active.clone(),
            CsvBaseConfig::new(b',', false),
        ));
        let mut r = CsvLensReader::new(config).unwrap();
        wait_till_ready(&r, &stream_active);
        let rows = r.get_rows(0, 50).unwrap().0;
        let expected = vec![
            Row::new(1, vec!["c1", " v1"]),
            Row::new(2, vec!["c2", " v2"]),
        ];
        assert_eq!(rows, expected);
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn test_small_delimiter(#[case] is_streaming: bool) {
        let stream_active = if is_streaming {
            Some(Arc::new(AtomicBool::new(true)))
        } else {
            None
        };
        let config = Arc::new(CsvConfig::new(
            "tests/data/small.bsv",
            stream_active.clone(),
            CsvBaseConfig::new(b'|', false),
        ));
        let mut r = CsvLensReader::new(config).unwrap();
        wait_till_ready(&r, &stream_active);
        let rows = r.get_rows(0, 50).unwrap().0;
        let expected = vec![Row::new(1, vec!["c1", "v1"]), Row::new(2, vec!["c2", "v2"])];
        assert_eq!(rows, expected);
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn test_irregular(#[case] is_streaming: bool) {
        let stream_active = if is_streaming {
            Some(Arc::new(AtomicBool::new(true)))
        } else {
            None
        };
        let config = Arc::new(CsvConfig::new(
            "tests/data/irregular.csv",
            stream_active.clone(),
            CsvBaseConfig::new(b',', false),
        ));
        let mut r = CsvLensReader::new(config).unwrap();
        wait_till_ready(&r, &stream_active);
        let rows = r.get_rows(0, 50).unwrap().0;
        let expected = vec![Row::new(1, vec!["c1"]), Row::new(2, vec!["c2", " v2"])];
        assert_eq!(rows, expected);
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn test_double_quoting_as_escape_chars(#[case] is_streaming: bool) {
        let stream_active = if is_streaming {
            Some(Arc::new(AtomicBool::new(true)))
        } else {
            None
        };
        let config = Arc::new(CsvConfig::new(
            "tests/data/good_double_quote.csv",
            stream_active.clone(),
            CsvBaseConfig::new(b',', false),
        ));
        let mut r = CsvLensReader::new(config).unwrap();
        wait_till_ready(&r, &stream_active);
        let rows = r.get_rows(0, 50).unwrap().0;
        let expected = vec![
            Row::new(1, vec!["1", "quote"]),
            Row::new(2, vec!["5", "Comma, comma"]),
        ];
        assert_eq!(rows, expected);
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn get_rows_unsorted_indices(#[case] is_streaming: bool) {
        let stream_active = if is_streaming {
            Some(Arc::new(AtomicBool::new(true)))
        } else {
            None
        };
        let config = Arc::new(CsvConfig::new(
            "tests/data/simple.csv",
            stream_active.clone(),
            CsvBaseConfig::new(b',', false),
        ));
        let mut r = CsvLensReader::new(config).unwrap();
        wait_till_ready(&r, &stream_active);
        let rows = r.get_rows_for_indices(&vec![1235, 1234]).unwrap().0;
        let expected = vec![
            Row::new(1236, vec!["A1236", "B1236"]),
            Row::new(1235, vec!["A1235", "B1235"]),
        ];
        assert_eq!(rows, expected);
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn test_streaming_100(#[case] is_streaming: bool) {
        let stream_active = if is_streaming {
            Some(Arc::new(AtomicBool::new(true)))
        } else {
            None
        };
        let config = Arc::new(CsvConfig::new(
            "tests/data/test_streaming_100.csv",
            stream_active.clone(),
            CsvBaseConfig::new(b',', false),
        ));
        let mut r = CsvLensReader::new(config).unwrap();
        wait_till_ready(&r, &stream_active);
        let rows = r.get_rows_for_indices(&vec![95]).unwrap().0;
        let expected = vec![Row::new(
            96,
            vec!["2020-05-05", "1000717", "717490024", "0", "train"],
        )];
        assert_eq!(rows, expected);
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn test_streaming_100_tsv(#[case] is_streaming: bool) {
        let stream_active = if is_streaming {
            Some(Arc::new(AtomicBool::new(true)))
        } else {
            None
        };
        let config = Arc::new(CsvConfig::new(
            "tests/data/test_streaming_100.tsv",
            stream_active.clone(),
            CsvBaseConfig::new(b'\t', false),
        ));
        let mut r = CsvLensReader::new(config).unwrap();
        wait_till_ready(&r, &stream_active);
        let rows = r.get_rows_for_indices(&vec![95]).unwrap().0;
        let expected = vec![Row::new(
            96,
            vec!["2020-05-05", "1000717", "717490024", "0", "train"],
        )];
        assert_eq!(rows, expected);
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn test_streaming_100_iterator(#[case] is_streaming: bool) {
        let stream_active = if is_streaming {
            Some(Arc::new(AtomicBool::new(true)))
        } else {
            None
        };
        let config = Arc::new(CsvConfig::new(
            "tests/data/test_streaming_100.csv",
            stream_active.clone(),
            CsvBaseConfig::new(b',', false),
        ));
        let mut iter = CsvlensRecordIterator::new(config).unwrap();
        iter.next();
        let position = iter.position();
        let mut expected = Position::new();
        // This is one case where record is not necessarily (line - 1)
        expected.set_byte(79);
        expected.set_line(2);
        expected.set_record(2);
        assert_eq!(*position, expected);
    }

    fn wait_till_ready(reader: &CsvLensReader, stream_active: &Option<Arc<AtomicBool>>) {
        // Wait till scanning starts. This will make the scanning use streaming / non-streaming
        // iterator based on the initial value of stream_active
        reader.wait_till_start_scanning();

        // Now turn off streaming mode if applicable so that the internal thread can finish
        stream_active
            .as_ref()
            .map(|x| x.store(false, Ordering::Relaxed));

        // Finally wait till internal thread is done
        reader.wait_internal();
    }
}
