use std::path::PathBuf;

use clap::Parser;

use crate::error::AppError;

/// crabplay - Local music player for macOS
#[derive(Parser, Debug)]
#[command(name = "crabplay", version, about)]
pub struct Args {
    /// Music directory to scan
    #[arg(short, long, default_value = ".")]
    pub dir: PathBuf,

    /// Output format (text / json) — used with --list
    #[arg(short, long, default_value = "text")]
    pub format: String,

    /// List tracks only without launching TUI
    #[arg(short, long, default_value_t = false)]
    pub list: bool,
}

impl Args {
    pub fn validate(&self) -> Result<(), AppError> {
        if !self.dir.exists() {
            return Err(AppError::Other(format!(
                "directory not found: '{}'",
                self.dir.display()
            )));
        }

        match self.format.as_str() {
            "text" | "json" => {}
            other => {
                return Err(AppError::Other(format!(
                    "unsupported format: '{other}' (expected 'text' or 'json')"
                )));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_accepts_known_formats() {
        let args = Args {
            dir: PathBuf::from("."),
            format: "json".to_string(),
            list: false,
        };
        assert!(args.validate().is_ok());
    }

    #[test]
    fn validate_rejects_unknown_format() {
        let args = Args {
            dir: PathBuf::from("."),
            format: "xml".to_string(),
            list: false,
        };
        assert!(args.validate().is_err());
    }
}
