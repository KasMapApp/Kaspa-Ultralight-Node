//! Output formatting: text (default) or `--json`.

use serde::Serialize;

#[derive(Copy, Clone, Debug)]
pub enum Format {
    Text,
    Json,
}

impl Format {
    pub fn emit<T: Serialize>(self, value: &T) -> anyhow::Result<()> {
        match self {
            Self::Text => Ok(()),  // callers render their own text above this
            Self::Json => {
                serde_json::to_writer_pretty(std::io::stdout(), value)?;
                println!();
                Ok(())
            }
        }
    }
}
