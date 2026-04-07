//! `ars` binary — short alias for the awkrs engine (`awkrs`).

use awkrs::Error;
use awkrs::run;

fn main() {
    let bin = env!("CARGO_BIN_NAME");
    match run(bin) {
        Ok(()) => {}
        Err(Error::Exit(code)) => std::process::exit(code),
        Err(e) => {
            eprintln!("{bin}: {e}");
            std::process::exit(1);
        }
    }
}
