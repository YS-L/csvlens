use csvlens::run_csvlens;

fn main() {
    let args_itr = std::env::args_os().skip(1);
    match run_csvlens(args_itr) {
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
