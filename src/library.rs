use crate::error::{KicadError, Result};
use regex::Regex;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static SYMBOL_WRITE_LOCK: Mutex<()> = Mutex::new(());

pub struct LibraryManager {
    output_path: PathBuf,
}

impl LibraryManager {
    pub fn new(output_path: &Path) -> Self {
        Self {
            output_path: output_path.to_path_buf(),
        }
    }

    /// Create necessary output directories
    pub fn create_directories(&self) -> Result<()> {
        // Create main output directory
        fs::create_dir_all(&self.output_path)
            .map_err(KicadError::Io)?;

        // Create .pretty directory for footprints
        let pretty_dir = self.output_path.join("nlbn.pretty");
        fs::create_dir_all(&pretty_dir)
            .map_err(KicadError::Io)?;

        // Create .3dshapes directory for 3D models
        let shapes_dir = self.output_path.join("nlbn.3dshapes");
        fs::create_dir_all(&shapes_dir)
            .map_err(KicadError::Io)?;

        Ok(())
    }

    /// Check if a component exists in the library file
    /// Note: This should only be called within a lock if used for write decisions
    pub fn component_exists(&self, lib_path: &Path, component_name: &str) -> Result<bool> {
        if !lib_path.exists() {
            return Ok(false);
        }

        let content = fs::read_to_string(lib_path)
            .map_err(KicadError::Io)?;

        // Check for v6 format
        let v6_pattern = format!(r#"\(symbol\s+"{}""#, regex::escape(component_name));
        if let Ok(re) = Regex::new(&v6_pattern) {
            if re.is_match(&content) {
                return Ok(true);
            }
        }

        // Check for v5 format
        let v5_pattern = format!(r"DEF\s+{}\s+", regex::escape(component_name));
        if let Ok(re) = Regex::new(&v5_pattern) {
            if re.is_match(&content) {
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// Add or update a component in the library file (thread-safe)
    pub fn add_or_update_component(&self, lib_path: &Path, component_name: &str, component_data: &str, overwrite: bool) -> Result<()> {
        // Lock to prevent concurrent writes and check-then-act race conditions
        let _lock = SYMBOL_WRITE_LOCK.lock().unwrap();

        // Check if component exists (within lock to prevent TOCTOU)
        let exists = if lib_path.exists() {
            let content = fs::read_to_string(lib_path)
                .map_err(KicadError::Io)?;

            let v6_pattern = format!(r#"\(symbol\s+"{}""#, regex::escape(component_name));
            if let Ok(re) = Regex::new(&v6_pattern) {
                re.is_match(&content)
            } else {
                false
            }
        } else {
            false
        };

        if exists && overwrite {
            // Update existing component
            self.update_component_internal(lib_path, component_name, component_data)?;
        } else if !exists {
            // Add new component
            self.add_component_internal(lib_path, component_data)?;
        }
        // If exists and !overwrite, do nothing

        Ok(())
    }

    /// Internal add component (assumes lock is held)
    fn add_component_internal(&self, lib_path: &Path, component_data: &str) -> Result<()> {
        let mut content = if lib_path.exists() {
            let existing = fs::read_to_string(lib_path)
                .map_err(KicadError::Io)?;
            existing.trim_end().trim_end_matches(')').to_string()
        } else {
            if component_data.contains("(symbol") {
                String::from("(kicad_symbol_lib\n  (version 20211014)\n  (generator nlbn)")
            } else {
                String::from("EESchema-LIBRARY Version 2.4\n#encoding utf-8")
            }
        };

        content.push('\n');
        content.push_str(component_data);

        if component_data.contains("(symbol") {
            content.push('\n');
            content.push(')');
        }
        content.push('\n');

        fs::write(lib_path, content)
            .map_err(KicadError::Io)?;

        Ok(())
    }

    /// Internal update component (assumes lock is held)
    fn update_component_internal(&self, lib_path: &Path, component_name: &str, new_data: &str) -> Result<()> {
        let content = fs::read_to_string(lib_path)
            .map_err(KicadError::Io)?;

        // Try v6 format: find symbol block by matching parentheses
        let search = format!(r#"(symbol "{}""#, component_name);
        if let Some(start) = content.find(&search) {
            // Walk back to find the opening '(' before "symbol"
            let block_start = content[..start].rfind('(').unwrap_or(start);
            // Count parentheses to find the matching close
            let mut depth = 0;
            let mut block_end = block_start;
            for (i, ch) in content[block_start..].char_indices() {
                match ch {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            block_end = block_start + i + 1;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            if block_end > block_start {
                let mut new_content = String::with_capacity(content.len());
                new_content.push_str(&content[..block_start]);
                new_content.push_str(new_data);
                new_content.push_str(&content[block_end..]);
                fs::write(lib_path, &new_content)
                    .map_err(KicadError::Io)?;
                return Ok(());
            }
        }

        // Try v5 format
        let v5_start = format!("DEF {} ", component_name);
        if let Some(start) = content.find(&v5_start) {
            if let Some(end_offset) = content[start..].find("ENDDEF") {
                let block_end = start + end_offset + "ENDDEF".len();
                // Skip trailing newline
                let block_end = if content[block_end..].starts_with('\n') { block_end + 1 } else { block_end };
                let mut new_content = String::with_capacity(content.len());
                new_content.push_str(&content[..start]);
                new_content.push_str(new_data);
                new_content.push_str(&content[block_end..]);
                fs::write(lib_path, &new_content)
                    .map_err(KicadError::Io)?;
                return Ok(());
            }
        }

        Err(KicadError::SymbolExport(format!("Component {} not found in library", component_name)).into())
    }

    /// Add a component to the library file
    pub fn add_component(&self, lib_path: &Path, component_data: &str) -> Result<()> {
        // Lock to prevent concurrent writes to the same symbol library file
        let _lock = SYMBOL_WRITE_LOCK.lock().unwrap();

        let mut content = if lib_path.exists() {
            // Read existing file and remove the closing parenthesis
            let existing = fs::read_to_string(lib_path)
                .map_err(KicadError::Io)?;
            // Remove trailing ')' and whitespace
            existing.trim_end().trim_end_matches(')').to_string()
        } else {
            // Create new library file with header (v6 format with proper formatting)
            if component_data.contains("(symbol") {
                // v6 format - match Python's formatting exactly
                String::from("(kicad_symbol_lib\n  (version 20211014)\n  (generator nlbn)")
            } else {
                // v5 format
                String::from("EESchema-LIBRARY Version 2.4\n#encoding utf-8")
            }
        };

        // Append component
        content.push('\n');
        content.push_str(component_data);

        // Add closing parenthesis for v6 format
        if component_data.contains("(symbol") {
            content.push('\n');
            content.push(')');
        }
        content.push('\n');

        fs::write(lib_path, content)
            .map_err(KicadError::Io)?;

        Ok(())
    }

    /// Update an existing component in the library file
    pub fn update_component(&self, lib_path: &Path, component_name: &str, new_data: &str) -> Result<()> {
        // Lock to prevent concurrent writes to the same symbol library file
        let _lock = SYMBOL_WRITE_LOCK.lock().unwrap();

        let content = fs::read_to_string(lib_path)
            .map_err(KicadError::Io)?;

        // Try v6 format: find symbol block by matching parentheses
        let search = format!(r#"(symbol "{}""#, component_name);
        if let Some(start) = content.find(&search) {
            let block_start = content[..start].rfind('(').unwrap_or(start);
            let mut depth = 0;
            let mut block_end = block_start;
            for (i, ch) in content[block_start..].char_indices() {
                match ch {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            block_end = block_start + i + 1;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            if block_end > block_start {
                let mut new_content = String::with_capacity(content.len());
                new_content.push_str(&content[..block_start]);
                new_content.push_str(new_data);
                new_content.push_str(&content[block_end..]);
                fs::write(lib_path, &new_content)
                    .map_err(KicadError::Io)?;
                return Ok(());
            }
        }

        // Try v5 format
        let v5_start = format!("DEF {} ", component_name);
        if let Some(start) = content.find(&v5_start) {
            if let Some(end_offset) = content[start..].find("ENDDEF") {
                let block_end = start + end_offset + "ENDDEF".len();
                let block_end = if content[block_end..].starts_with('\n') { block_end + 1 } else { block_end };
                let mut new_content = String::with_capacity(content.len());
                new_content.push_str(&content[..start]);
                new_content.push_str(new_data);
                new_content.push_str(&content[block_end..]);
                fs::write(lib_path, &new_content)
                    .map_err(KicadError::Io)?;
                return Ok(());
            }
        }

        Err(KicadError::SymbolExport(format!("Component {} not found in library", component_name)).into())
    }

    /// Write a footprint file
    pub fn write_footprint(&self, footprint_name: &str, data: &str) -> Result<PathBuf> {
        let pretty_dir = self.output_path.join("nlbn.pretty");
        let footprint_path = pretty_dir.join(format!("{}.kicad_mod", footprint_name));

        fs::write(&footprint_path, data)
            .map_err(KicadError::Io)?;

        log::info!("Wrote footprint: {}", footprint_path.display());

        Ok(footprint_path)
    }

    /// Write 3D model files
    pub fn write_3d_model(&self, model_name: &str, wrl_data: &str, step_data: &[u8]) -> Result<(PathBuf, PathBuf)> {
        let shapes_dir = self.output_path.join("nlbn.3dshapes");

        // Write VRML file
        let wrl_path = shapes_dir.join(format!("{}.wrl", model_name));
        fs::write(&wrl_path, wrl_data)
            .map_err(KicadError::Io)?;

        log::info!("Wrote VRML model: {}", wrl_path.display());

        // Write STEP file only if data is provided
        let step_path = shapes_dir.join(format!("{}.step", model_name));
        if !step_data.is_empty() {
            fs::write(&step_path, step_data)
                .map_err(KicadError::Io)?;
            log::info!("Wrote STEP model: {}", step_path.display());
        }

        Ok((wrl_path, step_path))
    }

    /// Write only VRML model (when STEP is not available)
    pub fn write_wrl_model(&self, model_name: &str, wrl_data: &str) -> Result<PathBuf> {
        let shapes_dir = self.output_path.join("nlbn.3dshapes");

        // Write VRML file
        let wrl_path = shapes_dir.join(format!("{}.wrl", model_name));
        fs::write(&wrl_path, wrl_data)
            .map_err(KicadError::Io)?;

        log::info!("Wrote VRML model: {}", wrl_path.display());

        Ok(wrl_path)
    }

    /// Write only STEP model
    pub fn write_step_model(&self, model_name: &str, step_data: &[u8]) -> Result<PathBuf> {
        let shapes_dir = self.output_path.join("nlbn.3dshapes");

        // Write STEP file
        let step_path = shapes_dir.join(format!("{}.step", model_name));
        fs::write(&step_path, step_data)
            .map_err(KicadError::Io)?;

        log::info!("Wrote STEP model: {}", step_path.display());

        Ok(step_path)
    }

    /// Get the symbol library path
    pub fn get_symbol_lib_path(&self, v5: bool) -> PathBuf {
        if v5 {
            self.output_path.join("nlbn.lib")
        } else {
            self.output_path.join("nlbn.kicad_sym")
        }
    }
}
