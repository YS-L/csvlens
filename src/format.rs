use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum NumberFormat {
    Thousands,  // e.g. 1,234,567.89
    Scientific, // e.g. 1.23e6
    Si,         // e.g. 1.23M  (k/M/B/T)
    Fixed,      // e.g. 23.51  (decimal places controlled by --precision)
}

impl NumberFormat {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "thousands" => Some(NumberFormat::Thousands),
            "scientific" | "sci" => Some(NumberFormat::Scientific),
            "si" => Some(NumberFormat::Si),
            "fixed" => Some(NumberFormat::Fixed),
            _ => None,
        }
    }

    /// Formats `value` according to this number format.
    /// `precision` controls decimal places for Scientific/Si/Fixed; defaults to 2 if None.
    /// For Thousands, `precision` truncates/pads decimal places if specified;
    /// if None, the original decimal representation is preserved.
    /// Returns the original string unchanged if `value` cannot be parsed as a finite number.
    pub fn apply(&self, value: &str, precision: Option<usize>) -> String {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return value.to_string();
        }
        let f: f64 = match trimmed.parse() {
            Ok(v) => v,
            Err(_) => return value.to_string(),
        };
        if f.is_nan() || f.is_infinite() {
            return value.to_string();
        }
        match self {
            NumberFormat::Thousands => format_thousands(f, precision),
            NumberFormat::Scientific => format_scientific(f, precision.unwrap_or(2)),
            NumberFormat::Si => format_human(f, precision.unwrap_or(2)),
            NumberFormat::Fixed => format_fixed(f, precision.unwrap_or(2)),
        }
    }
}

fn format_thousands(f: f64, precision: Option<usize>) -> String {
    let negative = f < 0.0;
    // Use precision-controlled formatting if specified, otherwise preserve original representation.
    let abs_str = match precision {
        Some(p) => format!("{:.prec$}", f.abs(), prec = p),
        None => format!("{}", f.abs()),
    };

    // Split on '.'
    let (int_part, dec_part) = if let Some(dot_pos) = abs_str.find('.') {
        (&abs_str[..dot_pos], Some(&abs_str[dot_pos..]))
    } else {
        (abs_str.as_str(), None)
    };

    // Insert commas every 3 digits from the right
    let int_chars: Vec<char> = int_part.chars().collect();
    let mut with_commas = String::new();
    let len = int_chars.len();
    for (i, ch) in int_chars.iter().enumerate() {
        if i > 0 && (len - i) % 3 == 0 {
            with_commas.push(',');
        }
        with_commas.push(*ch);
    }

    let result = match dec_part {
        Some(dec) => format!("{}{}", with_commas, dec),
        None => with_commas,
    };

    if negative {
        format!("-{}", result)
    } else {
        result
    }
}

fn format_scientific(f: f64, precision: usize) -> String {
    // Special-case zero: log10(0) is undefined (-∞), handle separately.
    if f == 0.0 {
        return format!("{:.prec$}e0", 0.0, prec = precision);
    }
    let negative = f < 0.0;
    let abs_f = f.abs();
    let exp = abs_f.log10().floor() as i32;
    let mantissa = abs_f / 10f64.powi(exp);
    let result = format!("{:.prec$}e{}", mantissa, exp, prec = precision);
    if negative {
        format!("-{}", result)
    } else {
        result
    }
}

fn format_fixed(f: f64, precision: usize) -> String {
    format!("{:.prec$}", f, prec = precision)
}

fn format_human(f: f64, precision: usize) -> String {
    let negative = f < 0.0;
    let abs_f = f.abs();

    let result = if abs_f >= 1e12 {
        format!("{:.prec$}T", abs_f / 1e12, prec = precision)
    } else if abs_f >= 1e9 {
        format!("{:.prec$}B", abs_f / 1e9, prec = precision)
    } else if abs_f >= 1e6 {
        format!("{:.prec$}M", abs_f / 1e6, prec = precision)
    } else if abs_f >= 1e3 {
        format!("{:.prec$}k", abs_f / 1e3, prec = precision)
    } else {
        // Below 1000: no suffix, still apply precision.
        format!("{:.prec$}", f, prec = precision)
    };

    // For values >= 1000 with a suffix, the abs value was used so prepend "-" if negative.
    if negative && abs_f >= 1e3 {
        format!("-{}", result)
    } else {
        result
    }
}

#[derive(Debug, Clone, Default)]
pub struct ColumnFormatConfig {
    named: HashMap<String, NumberFormat>,
    global: Option<NumberFormat>,
    /// Decimal places for Fixed/Scientific/Si formats. None uses each format's built-in default.
    precision: Option<usize>,
}

impl ColumnFormatConfig {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn insert_named(&mut self, column: String, fmt: NumberFormat) {
        self.named.insert(column, fmt);
    }

    pub fn set_global(&mut self, fmt: NumberFormat) {
        self.global = Some(fmt);
    }

    pub fn set_precision(&mut self, p: usize) {
        self.precision = Some(p);
    }

    pub fn precision(&self) -> Option<usize> {
        self.precision
    }

    /// Named config takes priority over global config.
    pub fn get(&self, column_name: &str) -> Option<&NumberFormat> {
        if let Some(fmt) = self.named.get(column_name) {
            return Some(fmt);
        }
        self.global.as_ref()
    }

    pub fn is_empty(&self) -> bool {
        self.named.is_empty() && self.global.is_none() && self.precision.is_none()
    }

    /// Returns true if there are any named (per-column) format configurations.
    pub fn has_named(&self) -> bool {
        !self.named.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- from_str ---

    #[test]
    fn from_str_thousands_aliases() {
        assert_eq!(NumberFormat::from_str("thousands"), Some(NumberFormat::Thousands));
        assert_eq!(NumberFormat::from_str("THOUSANDS"), Some(NumberFormat::Thousands));
    }

    #[test]
    fn from_str_scientific_aliases() {
        assert_eq!(NumberFormat::from_str("scientific"), Some(NumberFormat::Scientific));
        assert_eq!(NumberFormat::from_str("sci"),        Some(NumberFormat::Scientific));
        assert_eq!(NumberFormat::from_str("SCI"),        Some(NumberFormat::Scientific));
    }

    #[test]
    fn from_str_si_aliases() {
        assert_eq!(NumberFormat::from_str("si"), Some(NumberFormat::Si));
        assert_eq!(NumberFormat::from_str("SI"), Some(NumberFormat::Si));
    }

    #[test]
    fn from_str_fixed_aliases() {
        assert_eq!(NumberFormat::from_str("fixed"), Some(NumberFormat::Fixed));
        assert_eq!(NumberFormat::from_str("FIXED"), Some(NumberFormat::Fixed));
    }

    #[test]
    fn from_str_unknown() {
        assert!(NumberFormat::from_str("unknown").is_none());
        assert!(NumberFormat::from_str("").is_none());
    }

    // --- Thousands ---

    #[test]
    fn thousands_positive_integer() {
        assert_eq!(NumberFormat::Thousands.apply("1234567", None), "1,234,567");
    }

    #[test]
    fn thousands_negative_integer() {
        assert_eq!(NumberFormat::Thousands.apply("-1234", None), "-1,234");
    }

    #[test]
    fn thousands_float() {
        assert_eq!(NumberFormat::Thousands.apply("1234567.89", None), "1,234,567.89");
    }

    #[test]
    fn thousands_no_decimal() {
        assert_eq!(NumberFormat::Thousands.apply("1000", None), "1,000");
    }

    #[test]
    fn thousands_small_number() {
        assert_eq!(NumberFormat::Thousands.apply("999", None), "999");
    }

    #[test]
    fn thousands_negative_float() {
        assert_eq!(NumberFormat::Thousands.apply("-9876543.21", None), "-9,876,543.21");
    }

    #[test]
    fn thousands_with_precision() {
        assert_eq!(NumberFormat::Thousands.apply("1234567.89123", Some(2)), "1,234,567.89");
        assert_eq!(NumberFormat::Thousands.apply("1234567.89123", Some(0)), "1,234,568");
    }

    // --- Scientific ---

    #[test]
    fn scientific_positive() {
        let result = NumberFormat::Scientific.apply("20263.89", None);
        // mantissa = 2.026389, exp = 4 → "2.03e4"
        assert_eq!(result, "2.03e4");
    }

    #[test]
    fn scientific_with_precision() {
        assert_eq!(NumberFormat::Scientific.apply("20263.89", Some(4)), "2.0264e4");
        assert_eq!(NumberFormat::Scientific.apply("20263.89", Some(0)), "2e4");
    }

    #[test]
    fn scientific_negative() {
        let result = NumberFormat::Scientific.apply("-20263.89", None);
        assert_eq!(result, "-2.03e4");
    }

    #[test]
    fn scientific_small_decimal() {
        let result = NumberFormat::Scientific.apply("0.00123", None);
        // exp = floor(log10(0.00123)) = floor(-2.91) = -3
        // mantissa = 0.00123 / 1e-3 = 1.23
        assert_eq!(result, "1.23e-3");
    }

    #[test]
    fn scientific_zero() {
        assert_eq!(NumberFormat::Scientific.apply("0", None), "0.00e0");
    }

    // --- Si ---

    #[test]
    fn si_kilo() {
        assert_eq!(NumberFormat::Si.apply("1500", None), "1.50k");
    }

    #[test]
    fn si_mega() {
        assert_eq!(NumberFormat::Si.apply("1234567", None), "1.23M");
    }

    #[test]
    fn si_giga() {
        assert_eq!(NumberFormat::Si.apply("2500000000", None), "2.50B");
    }

    #[test]
    fn si_tera() {
        assert_eq!(NumberFormat::Si.apply("1500000000000", None), "1.50T");
    }

    #[test]
    fn si_negative() {
        assert_eq!(NumberFormat::Si.apply("-1500", None), "-1.50k");
    }

    #[test]
    fn si_small() {
        // Below 1000: no suffix, precision still applies.
        assert_eq!(NumberFormat::Si.apply("42", None), "42.00");
        assert_eq!(NumberFormat::Si.apply("42", Some(0)), "42");
        assert_eq!(NumberFormat::Si.apply("42.123", Some(1)), "42.1");
    }

    #[test]
    fn si_with_precision() {
        assert_eq!(NumberFormat::Si.apply("1500", Some(1)), "1.5k");
        assert_eq!(NumberFormat::Si.apply("1500", Some(0)), "2k");
    }

    // --- Fixed ---

    #[test]
    fn fixed_default_precision() {
        assert_eq!(NumberFormat::Fixed.apply("23.505744680851063", None), "23.51");
    }

    #[test]
    fn fixed_custom_precision() {
        assert_eq!(NumberFormat::Fixed.apply("23.505744680851063", Some(4)), "23.5057");
        assert_eq!(NumberFormat::Fixed.apply("23.505744680851063", Some(0)), "24");
    }

    #[test]
    fn fixed_negative() {
        assert_eq!(NumberFormat::Fixed.apply("-23.505744680851063", Some(2)), "-23.51");
    }

    #[test]
    fn fixed_non_numeric() {
        assert_eq!(NumberFormat::Fixed.apply("hello", None), "hello");
    }

    // --- Non-numeric / edge cases ---

    #[test]
    fn non_numeric_passthrough() {
        assert_eq!(NumberFormat::Thousands.apply("hello", None), "hello");
        assert_eq!(NumberFormat::Scientific.apply("N/A", None), "N/A");
        assert_eq!(NumberFormat::Si.apply("abc", None), "abc");
    }

    #[test]
    fn empty_string_passthrough() {
        assert_eq!(NumberFormat::Thousands.apply("", None), "");
        assert_eq!(NumberFormat::Scientific.apply("", None), "");
        assert_eq!(NumberFormat::Si.apply("", None), "");
    }

    // --- ColumnFormatConfig ---

    #[test]
    fn config_named_priority_over_global() {
        let mut cfg = ColumnFormatConfig::new();
        cfg.set_global(NumberFormat::Thousands);
        cfg.insert_named("price".to_string(), NumberFormat::Si);

        assert_eq!(cfg.get("price"), Some(&NumberFormat::Si));
        assert_eq!(cfg.get("other"), Some(&NumberFormat::Thousands));
    }

    #[test]
    fn config_is_empty() {
        let mut cfg = ColumnFormatConfig::new();
        assert!(cfg.is_empty());
        cfg.set_global(NumberFormat::Scientific);
        assert!(!cfg.is_empty());
    }

    #[test]
    fn config_none_when_no_match() {
        let cfg = ColumnFormatConfig::new();
        assert!(cfg.get("any_column").is_none());
    }

    // --- Real-world data scenarios ---

    // NSW electricity price data: values like 23.505744680851063
    #[test]
    fn fixed_nsw_electricity_prices() {
        let values = [
            ("23.505744680851063", "23.51"),
            ("19.44625",           "19.45"),
            ("281.0552083333333",  "281.06"),
            ("17.108541666666667", "17.11"),
            ("-5.25",              "-5.25"),
            ("0.0",                "0.00"),
        ];
        for (input, expected) in values {
            assert_eq!(
                NumberFormat::Fixed.apply(input, Some(2)),
                expected,
                "input: {input}"
            );
        }
    }

    // fixed with precision 0 (integer rounding)
    // Note: Rust uses round-half-to-even (banker's rounding), so 0.5 → "0", 1.5 → "2"
    #[test]
    fn fixed_precision_zero_rounding() {
        assert_eq!(NumberFormat::Fixed.apply("23.505", Some(0)), "24");
        assert_eq!(NumberFormat::Fixed.apply("23.499", Some(0)), "23");
        assert_eq!(NumberFormat::Fixed.apply("-23.505", Some(0)), "-24");
        assert_eq!(NumberFormat::Fixed.apply("0.4", Some(0)), "0");
        assert_eq!(NumberFormat::Fixed.apply("0.5", Some(0)), "0"); // banker's rounding: 0.5 → 0
        assert_eq!(NumberFormat::Fixed.apply("1.5", Some(0)), "2"); // banker's rounding: 1.5 → 2
    }

    // thousands + precision: financial data
    #[test]
    fn thousands_with_precision_financial() {
        assert_eq!(NumberFormat::Thousands.apply("1234567.8912", Some(2)), "1,234,567.89");
        assert_eq!(NumberFormat::Thousands.apply("1000000.0",   Some(0)), "1,000,000");
        assert_eq!(NumberFormat::Thousands.apply("999.999",     Some(1)), "1,000.0");
        assert_eq!(NumberFormat::Thousands.apply("-9876543.21", Some(2)), "-9,876,543.21");
    }

    // si + precision: large counts
    #[test]
    fn si_with_precision_large_counts() {
        assert_eq!(NumberFormat::Si.apply("1_500_000", None),    "1_500_000"); // underscores not valid f64
        assert_eq!(NumberFormat::Si.apply("1500000",   Some(1)), "1.5M");
        assert_eq!(NumberFormat::Si.apply("1500000",   Some(3)), "1.500M");
        assert_eq!(NumberFormat::Si.apply("999",       Some(2)), "999.00");    // < 1000, precision applies
        assert_eq!(NumberFormat::Si.apply("1000",      Some(2)), "1.00k");
        assert_eq!(NumberFormat::Si.apply("1000000000000", Some(1)), "1.0T");
    }

    // scientific + precision
    #[test]
    fn scientific_with_precision_variety() {
        assert_eq!(NumberFormat::Scientific.apply("0.00123",   Some(3)), "1.230e-3");
        assert_eq!(NumberFormat::Scientific.apply("1234567.0", Some(1)), "1.2e6");
        assert_eq!(NumberFormat::Scientific.apply("1.0",       Some(0)), "1e0");
        assert_eq!(NumberFormat::Scientific.apply("0",         Some(4)), "0.0000e0");
    }

    // non-numeric values must pass through unchanged regardless of format
    #[test]
    fn passthrough_all_formats() {
        let non_numeric = ["N/A", "null", "", " ", "2009-01-01", "hello world", "1e", "1.2.3"];
        for v in non_numeric {
            assert_eq!(NumberFormat::Fixed.apply(v, Some(2)),      v, "Fixed passthrough: {v:?}");
            assert_eq!(NumberFormat::Thousands.apply(v, None),     v, "Thousands passthrough: {v:?}");
            assert_eq!(NumberFormat::Scientific.apply(v, Some(2)), v, "Scientific passthrough: {v:?}");
            assert_eq!(NumberFormat::Si.apply(v, Some(2)),         v, "Si passthrough: {v:?}");
        }
    }

    // precision propagation via ColumnFormatConfig
    #[test]
    fn config_precision_propagates() {
        let mut cfg = ColumnFormatConfig::new();
        cfg.set_global(NumberFormat::Fixed);
        cfg.set_precision(3);

        let fmt = cfg.get("any_col").unwrap();
        assert_eq!(fmt.apply("23.505744680851063", cfg.precision()), "23.506");
    }
}
