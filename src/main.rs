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

use crate::runner::run_csvlens;

fn main() {
    match run_csvlens() {
        Err(e) => {
            println!("{e:?}");
            std::process::exit(1);
        }
        Ok(Some(selection)) => {
            println!("{selection}");
        }
        _ => {}
    }
}
