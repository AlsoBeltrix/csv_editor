//! A document: one table, the dialect it is saved in, its history and its file.

use std::path::{Path, PathBuf};

use crate::dialect::Dialect;
use crate::edit::{Edit, History};
use crate::io::{Error, ParseOptions, parse, write};
use crate::table::Table;

#[derive(Clone, Debug, Default)]
pub struct Document {
    pub table: Table,
    pub dialect: Dialect,
    pub history: History,
    pub path: Option<PathBuf>,
}

impl Document {
    /// An empty, unnamed document. The interface shows blank cells past the table's extent and
    /// grows the table when one is edited, so nothing blank is ever written.
    pub fn new() -> Document {
        Document::default()
    }

    pub fn from_bytes(bytes: &[u8], opts: &ParseOptions) -> Result<Document, Error> {
        let (table, dialect) = parse(bytes, opts)?;
        Ok(Document { table, dialect, history: History::default(), path: None })
    }

    pub fn open(path: impl AsRef<Path>, opts: &ParseOptions) -> Result<Document, Error> {
        let path = path.as_ref();
        let bytes = std::fs::read(path).map_err(|source| Error::Io { path: path.display().to_string(), source })?;
        let mut doc = Document::from_bytes(&bytes, opts)?;
        doc.path = Some(path.to_path_buf());
        Ok(doc)
    }

    /// The file name, or "Untitled".
    pub fn display_name(&self) -> String {
        self.path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Untitled".to_string())
    }

    pub fn is_dirty(&self) -> bool {
        self.history.is_dirty()
    }

    pub fn to_bytes(&self) -> (Vec<u8>, usize) {
        write(&self.table, &self.dialect)
    }

    /// Writes the document to `path` (through a temporary file in the same folder, so a failed
    /// write never leaves a truncated file), makes it the document's file and clears the dirty
    /// flag. Returns how many characters the encoding could not represent.
    pub fn save_to(&mut self, path: impl AsRef<Path>) -> Result<usize, Error> {
        let path = path.as_ref();
        let (bytes, lossy) = self.to_bytes();
        let io = |source: std::io::Error| Error::Io { path: path.display().to_string(), source };
        let mut tmp = path.as_os_str().to_owned();
        tmp.push(".csv-editor-tmp");
        let tmp = PathBuf::from(tmp);
        std::fs::write(&tmp, &bytes).map_err(io)?;
        if let Err(e) = std::fs::rename(&tmp, path) {
            let _ = std::fs::remove_file(&tmp);
            return Err(io(e));
        }
        self.path = Some(path.to_path_buf());
        self.history.mark_saved();
        Ok(lossy)
    }

    /// Applies an edit as one undo step. Returns whether anything changed.
    pub fn apply(&mut self, label: impl Into<String>, edit: Edit) -> bool {
        self.history.apply(&mut self.table, label, edit)
    }

    pub fn undo(&mut self) -> Option<String> {
        self.history.undo(&mut self.table)
    }

    pub fn redo(&mut self) -> Option<String> {
        self.history.redo(&mut self.table)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_edit_save_reopen() {
        let dir = std::env::temp_dir().join(format!("csv-editor-doc-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.csv");
        std::fs::write(&path, b"a,b\r\n1,2\r\n").unwrap();
        let mut doc = Document::open(&path, &ParseOptions::default()).unwrap();
        assert_eq!(doc.display_name(), "t.csv");
        assert!(!doc.is_dirty());
        assert!(doc.apply("Typing", Edit::set_cell(1, 1, "9")));
        assert!(doc.is_dirty());
        assert_eq!(doc.save_to(&path).unwrap(), 0);
        assert!(!doc.is_dirty());
        assert_eq!(std::fs::read(&path).unwrap(), b"a,b\r\n1,9\r\n");
        assert!(!dir.join("t.csv.csv-editor-tmp").exists());
        doc.undo();
        assert!(doc.is_dirty());
        let missing = Document::open(dir.join("missing.csv"), &ParseOptions::default());
        assert!(matches!(missing, Err(Error::Io { .. })));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn new_document_is_empty_and_untitled() {
        let doc = Document::new();
        assert_eq!(doc.display_name(), "Untitled");
        assert!(doc.table.is_empty());
        assert_eq!(doc.to_bytes().0, b"");
    }
}
