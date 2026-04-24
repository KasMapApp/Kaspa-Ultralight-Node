//! Output formatting: text (default) or `--json`.
//!
//! Reserved for v0.2. v0.1 subcommands render their own text inline; the
//! `Format::emit` helper here is a scaffold that will replace those bespoke
//! render blocks when we unify the output pipeline.

#![allow(dead_code)]

use serde::Serialize;

#[derive(Copy, Clone, Debug)]
pub enum Format {
    Text,
    Json,
}

impl Format {
    pub fn emit<T: Serialize>(self, value: &T) -> anyhow::Result<()> {
        match self {
            Self::Text => Ok(()), // callers render their own text above this
            Self::Json => {
                serde_json::to_writer_pretty(std::io::stdout(), value)?;
                println!();
                Ok(())
            }
        }
    }
}
