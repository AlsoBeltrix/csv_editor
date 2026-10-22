//! Times the data operations (sort, filter, find, replace, cleanup and their undo) on a large
//! file. Companion to `soak.rs`; numbers are recorded in `.agents/plans/csv-editor.md`.
//!
//! ```text
//! cargo run --release -p csv-editor-table --example ops_bench -- <file.csv>
//! ```
use std::time::Instant;

use csv_editor_table::{Case, CellRef, Document, FindOptions, Matcher, ParseOptions, Range, SortKind, SortSpec};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("usage: ops_bench <file.csv>")?;
    let mut doc = Document::open(&path, &ParseOptions::default())?;
    let rows = doc.table.row_count();
    let cols = doc.table.width();
    let all = Range::new(CellRef::new(0, 0), CellRef::new(rows.saturating_sub(1), cols.saturating_sub(1)));
    let mut t = Instant::now();
    let mut lap = |name: &str| {
        println!("{name:<28} {:>7.3} s", t.elapsed().as_secs_f64());
        t = Instant::now();
    };

    doc.sort(1, SortSpec { col: 2, ascending: true, kind: SortKind::Text });
    lap("sort text (city)");
    doc.undo();
    lap("undo sort");
    doc.sort(1, SortSpec { col: 3, ascending: false, kind: SortKind::Numeric });
    lap("sort numeric (amount)");
    doc.undo();
    lap("undo sort");

    // The grid's Contains filter, as view.rs does it today.
    let needle = "oslo".to_string();
    let n = (1..rows).filter(|&r| doc.table.get(r, 2).to_lowercase().contains(&needle.to_lowercase())).count();
    lap(&format!("filter contains ({n} rows)"));

    let m = Matcher::new(&FindOptions { query: "Person 1234".to_string(), ..Default::default() }).map_err(|e| e.to_string())?;
    let n = csv_editor_table::search::count_matches(&doc.table, &m, all, usize::MAX, &|_| true);
    lap(&format!("find count ({n} matches)"));
    let n = doc.replace_all(&m, "Someone", all, &|_| true);
    lap(&format!("replace all ({n} cells)"));
    doc.undo();
    lap("undo replace");

    let n = doc.trim_whitespace(all);
    lap(&format!("trim whitespace ({n} cells)"));
    let n = doc.change_case(all, Case::Upper);
    lap(&format!("upper case ({n} cells)"));
    doc.undo();
    lap("undo upper case");
    let n = doc.remove_duplicate_rows(1);
    lap(&format!("remove duplicates ({n} rows)"));
    doc.undo();
    lap("undo remove duplicates");
    let n = doc.remove_empty_rows(1);
    lap(&format!("remove empty rows ({n} rows)"));
    Ok(())
}
