//! # csvlens
//!
//! This crate allows you to use csvlens as a library.
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
mod app;
mod common;
mod csv;
mod delimiter;
mod find;
mod help;
mod history;
mod input;
mod io;
mod runner;
#[allow(dead_code)]
mod sort;
mod ui;
mod util;
mod view;
mod wrap;

pub use runner::run_csvlens;
