use std::fmt;

#[derive(Debug)]
pub enum ParseError {
    UnknownKeyword { keyword: String, card_text: String },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::UnknownKeyword { keyword, card_text } => write!(
                f,
                "unknown keyword {:?} in oracle text {:?}",
                keyword, card_text
            ),
        }
    }
}

impl std::error::Error for ParseError {}

mod oracle;
pub use oracle::parse_oracle_text;
