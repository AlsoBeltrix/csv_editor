//! Large-file soak: opens a file, saves it next to itself, checks the round trip and reports
//! timings. Peak memory is read from outside (see `.agents/plans/csv-editor.md`, slice 9).
//!
//! ```text
//! cargo run --release -p csv-editor-table --example soak -- <file.csv>
//! ```

use std::io::Read;
use std::time::Instant;

use csv_editor_table::{Document, ParseOptions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("usage: soak <file.csv>")?;
    let t = Instant::now();
    let mut doc = Document::open(&path, &ParseOptions::default())?;
    let open = t.elapsed();
    println!("open  {:>7.3} s  {} rows x {} cols", open.as_secs_f64(), doc.table.row_count(), doc.table.width());

    let out = format!("{path}.soak-out");
    let t = Instant::now();
    let lossy = doc.save_to(&out)?;
    let save = t.elapsed();
    let written = std::fs::metadata(&out)?.len();
    println!("save  {:>7.3} s  {written} bytes, {lossy} lossy", save.as_secs_f64());

    println!("round trip identical: {}", same_bytes(&path, &out)?);
    std::fs::remove_file(&out)?;
    // Give an outside poller a moment to read the peak working set.
    std::thread::sleep(std::time::Duration::from_millis(400));
    Ok(())
}

/// Whether two files have the same bytes, compared a megabyte at a time so the check itself
/// does not hold two copies of the file.
fn same_bytes(a: &str, b: &str) -> std::io::Result<bool> {
    let (mut a, mut b) = (std::fs::File::open(a)?, std::fs::File::open(b)?);
    let (mut buf_a, mut buf_b) = (vec![0u8; 1 << 20], vec![0u8; 1 << 20]);
    loop {
        let (na, nb) = (fill(&mut a, &mut buf_a)?, fill(&mut b, &mut buf_b)?);
        if na != nb || buf_a.get(..na) != buf_b.get(..nb) {
            return Ok(false);
        }
        if na == 0 {
            return Ok(true);
        }
    }
}

/// Reads until the buffer is full or the file ends; returns how much was read.
fn fill(file: &mut std::fs::File, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match file.read(buf.get_mut(n..).unwrap_or(&mut []))? {
            0 => break,
            k => n += k,
        }
    }
    Ok(n)
}
