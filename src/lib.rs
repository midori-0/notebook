//! notebook (`nb`): many small, timestamped, tagged markdown notes in one `.nb` file, plus `.nt`
//! link files that point back into a notebook.
//!
//! Commands stream a notebook one note at a time, so memory use stays flat however large the
//! notebook grows. Writers hold a lock and replace the file atomically.
//!
//! Layout, from the bottom up: `time` and `paths` are helpers; `format` reads and writes the text
//! format; `filter` and `link` select notes; `store` opens, locks and replaces files; `trash`
//! keeps removed notes; `read`, `write`, `remove` and `untag` are the commands; `cli` parses arguments.

mod cli;
mod env;
mod filter;
mod format;
mod link;
mod paths;
mod read;
mod remove;
mod store;
mod time;
mod trash;
mod untag;
mod write;

#[cfg(test)]
mod tests;

pub use cli::main;

type Res<T> = Result<T, Box<dyn std::error::Error>>;
