//! memtree checks, indexes, and diffs a markdown memory store: a directory tree of notes with
//! frontmatter, `[[name]]` links between them, and a `MEMORY.md` index with one line per note.

pub mod affected;
pub mod check;
pub mod finding;
pub mod frontmatter;
mod git;
pub mod index;
pub mod links;
pub mod note;
pub mod store;
