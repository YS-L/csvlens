use anyhow::{bail, Context, Result};

/// Delimiter behaviour as specified in the command line
pub enum Delimiter {
    /// Use the default delimiter (comma)
    Default,

    /// Use the specified delimiter
    Character(u8),

    /// Auto-detect the delimiter
    Auto,
}

impl Delimiter {
    /// Create a Delimiter by parsing the command line argument for the delimiter
    pub fn from_arg(delimiter_arg: &Option<String>, tab_separation: bool) -> Result<Self> {
        if tab_separation {
            return Ok(Delimiter::Character('\t'.try_into()?));
        }

        if let Some(s) = delimiter_arg {
            if s == "auto" {
                return Ok(Delimiter::Auto);
            }
            if s == r"\t" {
                return Ok(Delimiter::Character(b'\t'));
            }
            let mut chars = s.chars();
            let c = chars.next().context("Delimiter should not be empty")?;
            if !c.is_ascii() {
                bail!(
                    "Delimiter should be within the ASCII range: {} is too fancy",
                    c
                );
            }
            if chars.next().is_some() {
                bail!(
                    "Delimiter should be exactly one character (or \\t), got '{}'",
                    s
                );
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
