use reqwest::blocking::Client;
use crate::error::{EasyedaError, Result};
use crate::easyeda::models::{ComponentData, ApiResponse, Model3dInfo};

pub struct EasyedaApi {
    client: Client,
}

impl EasyedaApi {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .user_agent("nlbn/1.0.3")
                .build()
                .expect("Failed to create HTTP client"),
        }
    }

    pub fn get_component_data(&self, lcsc_id: &str) -> Result<ComponentData> {
        let url = format!(
            "https://easyeda.com/api/products/{}/components?version=6.4.19.5",
            lcsc_id
        );

        log::info!("Fetching component data for {}", lcsc_id);

        let response = self.client
            .get(&url)
            .send()
            .map_err(EasyedaError::ApiRequest)?;

        if !response.status().is_success() {
            return Err(EasyedaError::ComponentNotFound(lcsc_id.to_string()).into());
        }

        let api_response: ApiResponse = response.json()
            .map_err(|e| EasyedaError::InvalidData(format!("Failed to parse JSON: {}", e)))?;

        if !api_response.success {
            return Err(EasyedaError::ComponentNotFound(lcsc_id.to_string()).into());
        }

        let result = api_response.result
            .ok_or_else(|| EasyedaError::InvalidData("Missing result field".to_string()))?;

        // Parse dataStr - it's an object with a "shape" array
        let data_str_obj = result.data_str.as_ref()
            .ok_or_else(|| EasyedaError::InvalidData("Missing dataStr field".to_string()))?;

        log::debug!("data_str_obj type: {:?}", data_str_obj);

        // Extract bbox from head
        let bbox_x = data_str_obj.get("head")
            .and_then(|h| h.get("x"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let bbox_y = data_str_obj.get("head")
            .and_then(|h| h.get("y"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        log::debug!("Extracted bbox: x={}, y={}", bbox_x, bbox_y);

        let data_str = if let Some(shape_array) = data_str_obj.get("shape").and_then(|v| v.as_array()) {
            // Extract shape array as Vec<String>
            log::debug!("Found shape array with {} elements", shape_array.len());
            shape_array.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        } else {
            // Fallback: empty vector
            log::warn!("data_str_obj doesn't have shape array");
            vec![]
        };

        log::debug!("Final data_str has {} shapes", data_str.len());

        let title = result.title
            .ok_or_else(|| EasyedaError::InvalidData("Missing title field".to_string()))?;

        // Extract metadata from c_para and lcsc fields
        let manufacturer = data_str_obj.get("head")
            .and_then(|h| h.get("c_para"))
            .and_then(|cp| cp.get("BOM_Manufacturer"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let datasheet = result.lcsc.as_ref()
            .and_then(|lcsc| lcsc.get("url"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let jlc_id = data_str_obj.get("head")
            .and_then(|h| h.get("c_para"))
            .and_then(|cp| cp.get("BOM_JLCPCB Part Class"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        log::debug!("Extracted metadata: manufacturer={}, datasheet={}, jlc_id={}",
                   manufacturer, datasheet, jlc_id);

        // Parse package_detail - it's an object with a "dataStr" that has a "shape" array
        let (package_detail, package_bbox_x, package_bbox_y, model_3d) = if let Some(pkg) = result.package_detail {
            // Extract bbox from package head
            let pkg_bbox_x = pkg.get("dataStr")
                .and_then(|ds| ds.get("head"))
                .and_then(|h| h.get("x"))
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let pkg_bbox_y = pkg.get("dataStr")
                .and_then(|ds| ds.get("head"))
                .and_then(|h| h.get("y"))
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);

            log::debug!("Extracted package bbox: x={}, y={}", pkg_bbox_x, pkg_bbox_y);

            // Extract shape data
            let shapes = if let Some(pkg_data_str) = pkg.get("dataStr") {
                if let Some(shape_array) = pkg_data_str.get("shape").and_then(|v| v.as_array()) {
                    shape_array.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                } else {
                    vec![]
                }
            } else if pkg.is_array() {
                pkg.as_array()
                    .unwrap()
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            } else {
                vec![]
            };

            // Extract 3D model info from SVGNODE in shapes array
            let model_3d = Self::extract_3d_model_from_svgnode(&shapes);

            (shapes, pkg_bbox_x, pkg_bbox_y, model_3d)
        } else {
            (vec![], 0.0, 0.0, None)
        };

        Ok(ComponentData {
            lcsc_id: lcsc_id.to_string(),
            title,
            data_str,
            bbox_x,
            bbox_y,
            package_detail,
            package_bbox_x,
            package_bbox_y,
            model_3d,
            manufacturer,
            datasheet,
            jlc_id,
        })
    }

    fn extract_3d_model_from_svgnode(shapes: &[String]) -> Option<Model3dInfo> {
        // Look for SVGNODE in shapes array
        for shape in shapes {
            if shape.starts_with("SVGNODE~") {
                let parts: Vec<&str> = shape.split('~').collect();
                if parts.len() > 1 {
                    // Parse the JSON in the second part
                    if let Ok(svg_data) = serde_json::from_str::<serde_json::Value>(parts[1]) {
                        if let Some(attrs) = svg_data.get("attrs") {
                            // Check if this is a 3D model (c_etype == "outline3D")
                            if let Some(c_etype) = attrs.get("c_etype").and_then(|v| v.as_str()) {
                                if c_etype == "outline3D" {
                                    let uuid = attrs.get("uuid")
                                        .and_then(|v| v.as_str())
                                        .map(|s| s.to_string());
                                    let title = attrs.get("title")
                                        .and_then(|v| v.as_str())
                                        .map(|s| s.to_string());

                                    if let (Some(uuid), Some(title)) = (uuid, title) {
                                        return Some(Model3dInfo { uuid, title });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        None
    }

    pub fn download_3d_obj(&self, uuid: &str) -> Result<Vec<u8>> {
        let url = format!("https://modules.easyeda.com/3dmodel/{}", uuid);

        log::info!("Downloading 3D OBJ model: {}", uuid);

        let response = self.client
            .get(&url)
            .send()
            .map_err(EasyedaError::ApiRequest)?;

        if !response.status().is_success() {
            return Err(EasyedaError::InvalidData(format!("Failed to download OBJ: {}", uuid)).into());
        }

        let bytes = response.bytes()
            .map_err(EasyedaError::ApiRequest)?;

        Ok(bytes.to_vec())
    }

    pub fn download_3d_step(&self, uuid: &str) -> Result<Vec<u8>> {
        let url = format!("https://modules.easyeda.com/qAxj6KHrDKw4blvCG8QJPs7Y/{}", uuid);

        log::info!("Downloading 3D STEP model: {}", uuid);

        let response = self.client
            .get(&url)
            .send()
            .map_err(EasyedaError::ApiRequest)?;

        if !response.status().is_success() {
            return Err(EasyedaError::InvalidData(format!("Failed to download STEP: {}", uuid)).into());
        }

        let bytes = response.bytes()
            .map_err(EasyedaError::ApiRequest)?;

        Ok(bytes.to_vec())
    }
}

impl Default for EasyedaApi {
    fn default() -> Self {
        Self::new()
    }
}
