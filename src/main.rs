//! `awkrs` binary — same engine as `ars` (`src/bin/ars.rs`).

use awkrs::run;
use awkrs::Error;

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
