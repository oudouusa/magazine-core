use std::env;
use std::process;

mod ui_ext;

fn main() {
    if let Err(err) = ui_ext::run(env::args().skip(1).collect()) {
        eprintln!("error: {err}");
        eprintln!();
        eprintln!("{}", ui_ext::usage());
        process::exit(1);
    }
}
