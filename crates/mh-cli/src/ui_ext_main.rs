use std::env;
use std::process;

mod ui_ext;

fn main() {
    if let Err(error) = ui_ext::run(env::args().skip(1).collect()) {
        eprintln!("error: {error}");
        eprintln!();
        eprintln!("{}", ui_ext::usage());
        process::exit(1);
    }
}
