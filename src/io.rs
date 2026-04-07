//! Line-oriented input with a dedicated reader thread and bounded queue (producer–consumer).

use crate::error::{Error, Result};
use crossbeam_channel::bounded;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::thread;

/// Reads lines from stdin (`path == None`) or a file, prefetching up to `read_ahead` lines ahead of the consumer.
pub fn for_each_line(
    path: Option<&Path>,
    read_ahead: usize,
    mut on_line: impl FnMut(String) -> Result<()>,
) -> Result<()> {
    let cap = read_ahead.max(1);
    let (tx, rx) = bounded::<Option<String>>(cap);
    match path {
        None => {
            let reader = thread::spawn(move || {
                let stdin = std::io::stdin();
                let stdin = stdin.lock();
                for line in stdin.lines() {
                    let line = match line {
                        Ok(l) => l,
                        Err(_) => break,
                    };
                    if tx.send(Some(line)).is_err() {
                        break;
                    }
                }
                let _ = tx.send(None);
            });
            drain_lines(rx, &mut on_line)?;
            let _ = reader.join();
        }
        Some(p) => {
            let f = File::open(p).map_err(|e| Error::ProgramFile(p.to_path_buf(), e))?;
            let reader = thread::spawn(move || {
                for line in BufReader::new(f).lines() {
                    let line = match line {
                        Ok(l) => l,
                        Err(_) => break,
                    };
                    if tx.send(Some(line)).is_err() {
                        break;
                    }
                }
                let _ = tx.send(None);
            });
            drain_lines(rx, &mut on_line)?;
            let _ = reader.join();
        }
    }
    Ok(())
}

fn drain_lines(
    rx: crossbeam_channel::Receiver<Option<String>>,
    on_line: &mut impl FnMut(String) -> Result<()>,
) -> Result<()> {
    loop {
        match rx.recv() {
            Ok(Some(line)) => on_line(line)?,
            Ok(None) => break,
            Err(_) => break,
        }
    }
    Ok(())
}
