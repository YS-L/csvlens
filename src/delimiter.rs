use crate::errors::{CsvlensError, CsvlensResult};

/// Delimiter behaviour as specified in the command line
pub enum Delimiter {
    /// Use the default delimiter (comma)
    Default,

    /// Use tab as the delimiter
    Tab,

    /// Use the specified delimiter
    Character(u8),

    /// Auto-detect the delimiter
    Auto,
}

impl Delimiter {
    /// Create a Delimiter by parsing the command line argument for the delimiter
    pub fn from_arg(delimiter_arg: &Option<String>, tab_separation: bool) -> CsvlensResult<Self> {
        if tab_separation {
            return Ok(Delimiter::Tab);
        }

        if let Some(s) = delimiter_arg {
            if s == "auto" {
                return Ok(Delimiter::Auto);
            }
            if s == r"\t" {
                return Ok(Delimiter::Tab);
            }
            let mut chars = s.chars();
            let c = chars.next().ok_or_else(|| CsvlensError::DelimiterEmpty)?;
            if !c.is_ascii() {
                return Err(CsvlensError::DelimiterNotAscii(c));
            }
            if c == 't' {
                // commonly occurrs when argument is specified like "-d \t" without quotes
                return Ok(Delimiter::Tab);
            }
            if chars.next().is_some() {
                return Err(CsvlensError::DelimiterMultipleCharacters(s.clone()));
            }
            Ok(Delimiter::Character(c.try_into()?))
        } else {
            Ok(Delimiter::Default)
        }
    }
}

/// Sniff the delimiter from the file
pub fn sniff_delimiter(filename: &str) -> Option<u8> {
    let mut sniffer = csv_sniffer::Sniffer::new();
    sniffer.sample_size(csv_sniffer::SampleSize::Records(200));
    if let Ok(metadata) = sniffer.sniff_path(filename) {
        return Some(metadata.dialect.delimiter);
    }
    None
}
