mod local;

pub use local::LocalFileProvider;

pub struct DirEntry {
    pub name: String,
    pub kind: EntryKind,
    pub size: Option<i64>,
    pub mime_type: Option<String>,
}

pub enum EntryKind {
    Directory,
    File,
}
