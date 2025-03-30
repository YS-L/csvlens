use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use tempfile::NamedTempFile;

use crate::errors::{CsvlensError, CsvlensResult};

pub struct SeekableFile {
    filename: Option<String>,
    inner_file: Option<NamedTempFile>,
}

impl SeekableFile {
    pub fn new(maybe_filename: &Option<String>) -> CsvlensResult<SeekableFile> {
        let mut inner_file = NamedTempFile::new()?;
        let inner_file_res;

        if let Some(filename) = maybe_filename {
            let mut f = File::open(filename).map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => CsvlensError::FileNotFound(filename.clone()),
                _ => e.into(),
            })?;
            // If not seekable, it most likely is due to process substitution using
            // pipe - write out to a temp file to make it seekable
            if f.seek(SeekFrom::Start(0)).is_err() {
                Self::chunked_copy(&mut f, &mut inner_file)?;
                inner_file_res = Some(inner_file);
            } else {
                inner_file_res = None;
            }
        } else {
            // Handle input from stdin
            let mut stdin = std::io::stdin();
            Self::chunked_copy(&mut stdin, &mut inner_file)?;
            inner_file_res = Some(inner_file);
        }

        Ok(SeekableFile {
            filename: maybe_filename.clone(),
            inner_file: inner_file_res,
        })
    }

    pub fn filename(&self) -> &str {
        match &self.inner_file {
            Some(f) => f.path().to_str().unwrap(),
            _ => {
                // If data is from stdin, then inner_file must be there
                self.filename.as_ref().unwrap()
            }
        }
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
}
