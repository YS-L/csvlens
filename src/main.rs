use csvlens::run_csvlens;

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
