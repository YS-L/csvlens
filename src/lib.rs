//! # csvlens
//!
//! This crate allows you to use csvlens as a library.
//!
//! In your `Cargo.toml`, add the following:
//!
//! ```toml
//! [dependencies]
//! csvlens = { version = "0.11.0", default-features = false, features = ["clipboard"] }    
//! ```
//!
//! ## Example
//!
//! ```rust,no_run
//! use csvlens::run_csvlens;
//!
//! let out = run_csvlens(&["/path/to/your.csv"]).unwrap();
//! if let Some(selected_cell) = out {
//!     println!("Selected: {}", selected_cell);
//! }
//! ```
//!
//! ## Library Usage with options
//!
//! ```ignore
//! use csvlens::{run_csvlens_with_options, CsvlensOptions};
//!
//! let options = CsvlensOptions {
//!     filename: "/path/to/your.csv".to_string(),
//!     delimiter: Some("|".to_string()),
//!     ignore_case: true,
//!     debug: true,
//!     ..Default::default()
//! };
//! let out = run_csvlens_with_options(options).unwrap();
//! if let Some(selected_cell) = out {
//!     println!("Selected: {}", selected_cell);
//! }
//! ```
mod app;
mod columns_filter;
mod common;
mod csv;
mod delimiter;
pub mod errors;
mod find;
mod help;
mod history;
mod input;
mod io;
mod runner;
mod sort;
mod theme;
mod ui;
mod util;
mod view;
mod watch;
mod wrap;

pub use app::WrapMode;
pub use runner::CsvlensOptions;
pub use runner::run_csvlens;
pub use runner::run_csvlens_with_options;
