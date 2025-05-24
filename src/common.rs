use std::fmt;

#[derive(Clone, PartialEq, Eq, Hash, Copy, Debug)]
pub enum InputMode {
    Default,
    GotoLine,
    Find,
    Filter,
    FilterColumns,
    FreezeColumns,
    Option,
    Help,
}

impl fmt::Display for InputMode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
