use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tempfile::NamedTempFile;

use crate::csv::{CsvBaseConfig, CsvConfig, CsvlensRecordIterator};
use crate::errors::{CsvlensError, CsvlensResult};

pub struct SeekableFile {
    filename: Option<String>,
    inner_file: Option<NamedTempFile>,
    stream_active: Option<Arc<AtomicBool>>,
}

impl SeekableFile {
    pub fn new(
        maybe_filename: &Option<String>,
        no_streaming_stdin: bool,
    ) -> CsvlensResult<SeekableFile> {
        let inner_file = NamedTempFile::new()?;
        let inner_file_res;
        let mut stream_active = None;

        let mut stream_to_inner_file = || {
            let inner_path = inner_file.path().to_owned();

            // Thread to stream stdin to inner file
            let stream_active_flag = Arc::new(AtomicBool::new(true));
            let _stream_active_flag = stream_active_flag.clone();
            let _inner_path = inner_path.clone();
            std::thread::spawn(move || {
                let mut stdin = std::io::stdin();
                Self::chunked_copy_to_path(&mut stdin, _inner_path).unwrap();
                _stream_active_flag.store(false, Ordering::Relaxed);
            });
            stream_active = Some(stream_active_flag);

            // Thread to wait for the headers to be available. This is needed because once App is
            // started, it will immediately read the headers from the file. For slowly streaming
            // inputs, the headers might not be available yet.
            let _stream_active = stream_active.clone();
            let handle = std::thread::spawn(move || {
                // The delimiter here can be just an approximation since we just need to make sure
                // the header row as a whole is ready. Set no_headers: true to yield the header row
                // as a record.
                let base_config = CsvBaseConfig::new(b',', true);
                let path = inner_path.to_str().unwrap();
                let config = CsvConfig::new(path, _stream_active, base_config);
                let mut record_iterator = CsvlensRecordIterator::new(Arc::new(config)).unwrap();
                record_iterator.next();
            });
            handle.join().unwrap();
        };

        let copy_to_inner_file = || {
            let inner_path = inner_file.path().to_owned();
            let mut stdin = std::io::stdin();
            Self::chunked_copy_to_path(&mut stdin, inner_path).unwrap();
        };

        let mut prepare_inner_file = || {
            if no_streaming_stdin {
                copy_to_inner_file()
            } else {
                stream_to_inner_file()
            }
        };

        if let Some(filename) = maybe_filename {
            let mut f = File::open(filename).map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => CsvlensError::FileNotFound(filename.clone()),
                _ => e.into(),
            })?;
            // If not seekable, it most likely is due to process substitution using
            // pipe - write out to a temp file to make it seekable
            if f.seek(SeekFrom::Start(0)).is_err() {
                prepare_inner_file();
                inner_file_res = Some(inner_file);
            } else {
                inner_file_res = None;
            }
        } else {
            // Handle input from stdin
            prepare_inner_file();
            inner_file_res = Some(inner_file);
        }

        Ok(SeekableFile {
            filename: maybe_filename.clone(),
            inner_file: inner_file_res,
            stream_active,
        })
    }

    pub fn filename(&self) -> &str {
        if let Some(f) = &self.inner_file {
            f.path().to_str().unwrap()
        } else {
            // If data is from stdin, then inner_file must be there
            self.filename.as_ref().unwrap()
        }
    }

    pub fn stream_active(&self) -> &Option<Arc<AtomicBool>> {
        &self.stream_active
    }

    fn chunked_copy<R: Read, W: Write>(source: &mut R, dest: &mut W) -> CsvlensResult<usize> {
        let mut total_copied = 0;
        let mut buffer = vec![0; 1_000_000];
        loop {
            let n = source.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            let n_written = dest.write(&buffer[..n])?;
            total_copied += n_written;
        }
        Ok(total_copied)
    }

    fn chunked_copy_to_path<R: Read>(
        source: &mut R,
        path: impl AsRef<std::path::Path>,
    ) -> CsvlensResult<usize> {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)?;

        SeekableFile::chunked_copy(source, &mut file)
    }
}
