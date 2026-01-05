use regex::Regex;

#[derive(Debug)]
pub struct ColumnsFilter {
    pattern: Regex,
    indices: Vec<usize>,
    filtered_headers: Vec<String>,
    filtered_flags: Vec<bool>,
    disabled_because_no_match: bool,
}

impl ColumnsFilter {
    pub fn new(pattern: Regex, headers: &[String]) -> Self {
        let mut indices = vec![];
        let mut filtered_headers: Vec<String> = vec![];
        let mut filtered_flags: Vec<bool> = vec![];
        for (i, header) in headers.iter().enumerate() {
            if pattern.is_match(header) {
                indices.push(i);
                filtered_headers.push(header.clone());
                filtered_flags.push(true);
            } else {
                filtered_flags.push(false);
            }
        }
        let disabled_because_no_match;
        if indices.is_empty() {
            indices = (0..headers.len()).collect();
            filtered_headers = headers.into();
            disabled_because_no_match = true;
        } else {
            disabled_because_no_match = false;
        }
        Self {
            pattern,
            indices,
            filtered_headers,
            filtered_flags,
            disabled_because_no_match,
        }
    }

    pub fn filtered_headers(&self) -> &Vec<String> {
        &self.filtered_headers
    }

    pub fn indices(&self) -> &Vec<usize> {
        &self.indices
    }

    pub fn pattern(&self) -> Regex {
        self.pattern.to_owned()
    }

    pub fn num_filtered(&self) -> usize {
        self.indices.len()
    }

    pub fn disabled_because_no_match(&self) -> bool {
        self.disabled_because_no_match
    }

    pub fn is_column_filtered(&self, index: usize) -> bool {
        self.filtered_flags.get(index).cloned().unwrap_or(false)
    }
}
