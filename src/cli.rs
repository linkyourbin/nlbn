use crate::error::{AppError, Result};
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "nlbn")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "Fast EasyEDA/LCSC to KiCad converter with parallel downloads", long_about = None)]
pub struct Cli {
    /// LCSC component ID (e.g., C2040)
    #[arg(long, value_name = "ID", conflicts_with = "batch")]
    pub lcsc_id: Option<String>,

    /// Batch mode: read LCSC IDs from a file (one ID per line)
    #[arg(long, value_name = "FILE", conflicts_with = "lcsc_id")]
    pub batch: Option<PathBuf>,

    /// Convert symbol only
    #[arg(long)]
    pub symbol: bool,

    /// Convert footprint only
    #[arg(long)]
    pub footprint: bool,

    /// Convert 3D model only
    #[arg(long = "3d")]
    pub model_3d: bool,

    /// Convert all (symbol + footprint + 3D model)
    #[arg(long)]
    pub full: bool,

    /// Output directory path
    #[arg(short, long, default_value = ".")]
    pub output: PathBuf,

    /// Base library name used under --output when explicit library targets are not provided
    #[arg(long, value_name = "NAME")]
    pub lib_name: Option<String>,

    /// Existing symbol library file to append/update (.kicad_sym or .lib)
    #[arg(long, value_name = "FILE")]
    pub symbol_lib: Option<PathBuf>,

    /// Existing footprint library directory to append/update (.pretty)
    #[arg(long, value_name = "DIR")]
    pub footprint_lib: Option<PathBuf>,

    /// Existing 3D model library directory to append/update (.3dshapes)
    #[arg(long, value_name = "DIR")]
    pub model_lib: Option<PathBuf>,

    /// Overwrite existing components
    #[arg(long)]
    pub overwrite: bool,

    /// Use KiCad v5 legacy format
    #[arg(long)]
    pub v5: bool,

    /// Use project-relative paths (KIPRJMOD) instead of global paths for 3D models
    #[arg(long)]
    pub project_relative: bool,

    /// Enable debug logging
    #[arg(long)]
    pub debug: bool,

    /// Continue on error in batch mode (skip failed components)
    #[arg(long)]
    pub continue_on_error: bool,

    /// Number of parallel downloads in batch mode (default: 4)
    #[arg(long, default_value = "4")]
    pub parallel: usize,
}

impl Cli {
    pub fn validate(&self) -> Result<()> {
        // Check if at least one ID source is provided
        if self.lcsc_id.is_none() && self.batch.is_none() {
            return Err(AppError::Other(
                "Either --lcsc-id or --batch must be specified".to_string(),
            ));
        }

        // Validate LCSC ID format if provided
        if let Some(ref id) = self.lcsc_id {
            if !id.starts_with('C') || id.len() < 2 {
                return Err(AppError::Easyeda(
                    crate::error::EasyedaError::InvalidLcscId(id.clone()),
                ));
            }
        }

        // Check if at least one conversion option is selected
        if !self.symbol && !self.footprint && !self.model_3d && !self.full {
            return Err(AppError::Other(
                "At least one conversion option must be specified (--symbol, --footprint, --3d, or --full)".to_string()
            ));
        }

        if let Some(lib_name) = &self.lib_name {
            if lib_name.trim().is_empty() {
                return Err(AppError::Other("--lib-name must not be empty".to_string()));
            }
        }

        if let Some(symbol_lib) = &self.symbol_lib {
            let expected_suffix = if self.v5 { ".lib" } else { ".kicad_sym" };
            if !path_ends_with(symbol_lib, expected_suffix) {
                return Err(AppError::Other(format!(
                    "--symbol-lib must point to a {} file when --v5 is {}",
                    expected_suffix, self.v5
                )));
            }
        }

        if let Some(footprint_lib) = &self.footprint_lib {
            if !path_ends_with(footprint_lib, ".pretty") {
                return Err(AppError::Other(
                    "--footprint-lib must point to a .pretty directory".to_string(),
                ));
            }
        }

        if let Some(model_lib) = &self.model_lib {
            if !path_ends_with(model_lib, ".3dshapes") {
                return Err(AppError::Other(
                    "--model-lib must point to a .3dshapes directory".to_string(),
                ));
            }
        }

        Ok(())
    }

    /// Get list of LCSC IDs to process (either single ID or from batch file)
    pub fn get_lcsc_ids(&self) -> Result<Vec<String>> {
        if let Some(ref id) = self.lcsc_id {
            // Single ID mode
            Ok(vec![id.clone()])
        } else if let Some(ref batch_file) = self.batch {
            // Batch mode: extract all LCSC IDs (C + digits) from file
            use regex::Regex;
            use std::fs;

            let content = fs::read_to_string(batch_file)
                .map_err(|e| AppError::Other(format!("Failed to open batch file: {}", e)))?;

            let re = Regex::new(r"C\d+").unwrap();
            let ids: Vec<String> = re
                .find_iter(&content)
                .map(|m| m.as_str().to_string())
                .collect();

            if ids.is_empty() {
                return Err(AppError::Other(
                    "No valid LCSC IDs found in batch file".to_string(),
                ));
            }

            log::info!("Loaded {} LCSC IDs from batch file", ids.len());
            Ok(ids)
        } else {
            Err(AppError::Other("No LCSC ID source specified".to_string()))
        }
    }

    pub fn kicad_version(&self) -> KicadVersion {
        if self.v5 {
            KicadVersion::V5
        } else {
            KicadVersion::V6
        }
    }

    pub fn resolved_lib_name(&self) -> String {
        self.lib_name.clone().unwrap_or_else(|| {
            self.output
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("nlbn")
                .to_string()
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KicadVersion {
    V5,
    V6,
}

fn path_ends_with(path: &std::path::Path, suffix: &str) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.ends_with(suffix))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn rejects_v6_symbol_override_with_legacy_extension() {
        let cli = Cli::try_parse_from([
            "nlbn",
            "--lcsc-id",
            "C2040",
            "--symbol",
            "--symbol-lib",
            "custom.lib",
        ])
        .unwrap();

        let err = cli.validate().unwrap_err().to_string();
        assert!(err.contains("--symbol-lib must point to a .kicad_sym file"));
    }

    #[test]
    fn rejects_invalid_footprint_override() {
        let cli = Cli::try_parse_from([
            "nlbn",
            "--lcsc-id",
            "C2040",
            "--full",
            "--footprint-lib",
            "custom.dir",
        ])
        .unwrap();

        let err = cli.validate().unwrap_err().to_string();
        assert!(err.contains("--footprint-lib must point to a .pretty directory"));
    }

    #[test]
    fn rejects_invalid_model_override() {
        let cli = Cli::try_parse_from([
            "nlbn",
            "--lcsc-id",
            "C2040",
            "--3d",
            "--model-lib",
            "models.dir",
        ])
        .unwrap();

        let err = cli.validate().unwrap_err().to_string();
        assert!(err.contains("--model-lib must point to a .3dshapes directory"));
    }
}
