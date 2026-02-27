use thiserror::Error;

#[derive(Error, Debug)]
pub enum EasyedaError {
    #[error("API request failed: {0}")]
    ApiRequest(#[from] reqwest::Error),

    #[error("Invalid LCSC ID format: {0}")]
    InvalidLcscId(String),

    #[error("Component not found: {0}")]
    ComponentNotFound(String),

    #[error("Failed to parse JSON response: {0}")]
    JsonParse(#[from] serde_json::Error),

    #[error("Invalid component data: {0}")]
    InvalidData(String),
}

#[derive(Error, Debug)]
pub enum KicadError {
    #[error("Failed to export symbol: {0}")]
    SymbolExport(String),

    #[error("Failed to export footprint: {0}")]
    FootprintExport(String),

    #[error("Failed to export 3D model: {0}")]
    ModelExport(String),

    #[error("Invalid KiCad version")]
    InvalidVersion,

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Error, Debug)]
pub enum ConversionError {
    #[error("Invalid coordinate: {0}")]
    InvalidCoordinate(String),

    #[error("Invalid unit conversion: {0}")]
    InvalidUnit(String),

    #[error("SVG path parse error: {0}")]
    SvgParse(String),

    #[error("Arc conversion failed: {0}")]
    ArcConversion(String),
}

#[derive(Error, Debug)]
pub enum AppError {
    #[error(transparent)]
    Easyeda(#[from] EasyedaError),

    #[error(transparent)]
    Kicad(#[from] KicadError),

    #[error(transparent)]
    Conversion(#[from] ConversionError),

    #[error("Regex error: {0}")]
    Regex(#[from] regex::Error),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, AppError>;
