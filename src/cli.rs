use clap::Parser;
use std::path::PathBuf;
use crate::error::{AppError, Result};

#[derive(Parser, Debug)]
#[command(name = "nlbn")]
#[command(version = "1.0.9")]
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

    /// Overwrite existing components
    #[arg(long)]
    pub overwrite: bool,

    /// Use KiCad v5 legacy format
    #[arg(long)]
    pub v5: bool,

    /// Use global paths (KICAD6_3DMODEL_DIR) instead of project-relative paths (KIPRJMOD) for 3D models
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
                "Either --lcsc-id or --batch must be specified".to_string()
            ));
        }

        // Validate LCSC ID format if provided
        if let Some(ref id) = self.lcsc_id {
            if !id.starts_with('C') || id.len() < 2 {
                return Err(AppError::Easyeda(
                    crate::error::EasyedaError::InvalidLcscId(id.clone())
                ));
            }
        }

        // Check if at least one conversion option is selected
        if !self.symbol && !self.footprint && !self.model_3d && !self.full {
            return Err(AppError::Other(
                "At least one conversion option must be specified (--symbol, --footprint, --3d, or --full)".to_string()
            ));
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
            use std::fs;
            use regex::Regex;

            let content = fs::read_to_string(batch_file)
                .map_err(|e| AppError::Other(format!("Failed to open batch file: {}", e)))?;

            let re = Regex::new(r"C\d+").unwrap();
            let ids: Vec<String> = re.find_iter(&content)
                .map(|m| m.as_str().to_string())
                .collect();

            if ids.is_empty() {
                return Err(AppError::Other("No valid LCSC IDs found in batch file".to_string()));
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KicadVersion {
    V5,
    V6,
}
