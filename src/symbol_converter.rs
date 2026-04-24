use crate::cli::Cli;
use crate::converter::{Converter, sanitize_name};
use crate::easyeda::{ComponentData, SymbolImporter};
use crate::error::Result;
use crate::kicad;
use crate::library::{LibraryManager, SymbolWriteStatus};

pub fn convert_symbol(
    args: &Cli,
    component_data: &ComponentData,
    lib_manager: &LibraryManager,
    lcsc_id: &str,
) -> Result<()> {
    let ee_symbol = SymbolImporter::parse(&component_data.data_str)?;

    // Use LCSC ID as unique identifier to prevent name collisions
    let component_name = format!("{}_{}", sanitize_name(&component_data.title), lcsc_id);
    let footprint_name = component_name.clone();

    let mut ki_symbol = kicad::KiSymbol {
        name: component_name.clone(),
        reference: ee_symbol.prefix.clone(),
        value: component_data.title.clone(),
        description: component_data.description.clone(),
        footprint: format!("{}:{}", lib_manager.footprint_lib_name(), footprint_name),
        datasheet: component_data.datasheet.clone(),
        manufacturer: component_data.manufacturer.clone(),
        lcsc_id: component_data.lcsc_id.clone(),
        jlc_id: component_data.jlc_id.clone(),
        pins: Vec::new(),
        rectangles: Vec::new(),
        circles: Vec::new(),
        arcs: Vec::new(),
        polylines: Vec::new(),
        texts: Vec::new(),
    };

    // Convert pins with bbox adjustment
    let _converter = Converter::new(args.kicad_version());

    log::debug!(
        "bbox_x = {}, bbox_y = {}",
        component_data.bbox_x,
        component_data.bbox_y
    );

    for ee_pin in &ee_symbol.pins {
        let adjusted_x = ee_pin.x - component_data.bbox_x;
        let adjusted_y = ee_pin.y - component_data.bbox_y;

        if ee_pin.name.contains("PG10") {
            log::info!(
                "PG10 pin: raw x={}, y={}, adjusted x={}, y={}, final y={}",
                ee_pin.x,
                ee_pin.y,
                adjusted_x,
                adjusted_y,
                -adjusted_y
            );
        }

        // Log pins with unusual length
        if ee_pin.length >= 100.0 {
            log::warn!(
                "Pin {} ({}) has unusual length: {}",
                ee_pin.number,
                ee_pin.name,
                ee_pin.length
            );
        }

        ki_symbol.pins.push(kicad::KiPin {
            number: ee_pin.number.clone(),
            name: ee_pin.name.clone(),
            pin_type: kicad::PinType::from_easyeda(&ee_pin.electric_type),
            style: if ee_pin.dot {
                kicad::PinStyle::Inverted
            } else if ee_pin.clock {
                kicad::PinStyle::Clock
            } else {
                kicad::PinStyle::Line
            },
            pos_x: adjusted_x,
            pos_y: -adjusted_y, // Back to negation to test
            rotation: ee_pin.rotation,
            length: ee_pin.length,
        });
    }

    // Convert rectangles with bbox adjustment
    for (_idx, ee_rect) in ee_symbol.rectangles.iter().enumerate() {
        let adjusted_x = ee_rect.x - component_data.bbox_x;
        let adjusted_y = component_data.bbox_y - ee_rect.y; // bbox_y - pos_y
        let adjusted_x2 = (ee_rect.x + ee_rect.width) - component_data.bbox_x;
        let adjusted_y2 = component_data.bbox_y - (ee_rect.y + ee_rect.height); // bbox_y - (pos_y + height)

        ki_symbol.rectangles.push(kicad::KiRectangle {
            x1: adjusted_x,
            y1: adjusted_y, // No negation
            x2: adjusted_x2,
            y2: adjusted_y2, // No negation
            stroke_width: ee_rect.stroke_width,
            fill: true,
        });
    }

    // Convert circles with bbox adjustment
    for ee_circle in &ee_symbol.circles {
        let adjusted_cx = ee_circle.cx - component_data.bbox_x;
        let adjusted_cy = component_data.bbox_y - ee_circle.cy; // bbox_y - pos_y

        ki_symbol.circles.push(kicad::KiCircle {
            cx: adjusted_cx,
            cy: adjusted_cy, // No negation
            radius: ee_circle.radius,
            stroke_width: ee_circle.stroke_width,
            fill: ee_circle.fill,
        });
    }

    // Convert ellipses with bbox adjustment
    // If rx == ry, treat as circle; otherwise, approximate as circle with average radius
    for ee_ellipse in &ee_symbol.ellipses {
        let adjusted_cx = ee_ellipse.cx - component_data.bbox_x;
        let adjusted_cy = component_data.bbox_y - ee_ellipse.cy; // bbox_y - pos_y

        // Use average of rx and ry as radius (or just rx if they're equal)
        let radius = (ee_ellipse.rx + ee_ellipse.ry) / 2.0;

        ki_symbol.circles.push(kicad::KiCircle {
            cx: adjusted_cx,
            cy: adjusted_cy, // No negation
            radius,
            stroke_width: ee_ellipse.stroke_width,
            fill: ee_ellipse.fill,
        });
    }

    // Convert arcs with bbox adjustment
    // EeArc has center (x, y), radius, start_angle, end_angle
    // KiArc needs start, mid, and end points
    for ee_arc in &ee_symbol.arcs {
        // Convert angles from degrees to radians
        let start_angle_rad = ee_arc.start_angle.to_radians();
        let end_angle_rad = ee_arc.end_angle.to_radians();

        // Calculate start point
        let start_x = ee_arc.x + ee_arc.radius * start_angle_rad.cos();
        let start_y = ee_arc.y + ee_arc.radius * start_angle_rad.sin();

        // Calculate end point
        let end_x = ee_arc.x + ee_arc.radius * end_angle_rad.cos();
        let end_y = ee_arc.y + ee_arc.radius * end_angle_rad.sin();

        // Calculate midpoint angle (halfway between start and end)
        let mid_angle_rad = (start_angle_rad + end_angle_rad) / 2.0;
        let mid_x = ee_arc.x + ee_arc.radius * mid_angle_rad.cos();
        let mid_y = ee_arc.y + ee_arc.radius * mid_angle_rad.sin();

        // Apply bbox adjustment
        let adjusted_start_x = start_x - component_data.bbox_x;
        let adjusted_start_y = component_data.bbox_y - start_y;
        let adjusted_mid_x = mid_x - component_data.bbox_x;
        let adjusted_mid_y = component_data.bbox_y - mid_y;
        let adjusted_end_x = end_x - component_data.bbox_x;
        let adjusted_end_y = component_data.bbox_y - end_y;

        ki_symbol.arcs.push(kicad::SymbolKiArc {
            start_x: adjusted_start_x,
            start_y: adjusted_start_y,
            mid_x: adjusted_mid_x,
            mid_y: adjusted_mid_y,
            end_x: adjusted_end_x,
            end_y: adjusted_end_y,
            stroke_width: ee_arc.stroke_width,
        });
    }

    // Convert polylines with bbox adjustment
    for ee_polyline in &ee_symbol.polylines {
        let adjusted_points: Vec<(f64, f64)> = ee_polyline
            .points
            .iter()
            .map(|(x, y)| {
                let adj_x = x - component_data.bbox_x;
                let adj_y = component_data.bbox_y - y; // bbox_y - pos_y
                (adj_x, adj_y) // No negation
            })
            .collect();

        ki_symbol.polylines.push(kicad::KiPolyline {
            points: adjusted_points,
            stroke_width: ee_polyline.stroke_width,
            fill: false,
        });
    }

    // Convert polygons to polylines with bbox adjustment
    for ee_polygon in &ee_symbol.polygons {
        let adjusted_points: Vec<(f64, f64)> = ee_polygon
            .points
            .iter()
            .map(|(x, y)| {
                let adj_x = x - component_data.bbox_x;
                let adj_y = component_data.bbox_y - y; // bbox_y - pos_y
                (adj_x, adj_y) // No negation
            })
            .collect();

        ki_symbol.polylines.push(kicad::KiPolyline {
            points: adjusted_points,
            stroke_width: ee_polygon.stroke_width,
            fill: ee_polygon.fill,
        });
    }

    // Convert paths to polylines with bbox adjustment
    // Parse SVG path commands (M, L, Z) and convert to polylines
    for ee_path in &ee_symbol.paths {
        let path_str = &ee_path.path_data;
        let tokens: Vec<&str> = path_str.split_whitespace().collect();
        let mut points = Vec::new();
        let mut i = 0;

        while i < tokens.len() {
            let token = tokens[i];
            match token {
                "M" | "L" => {
                    // Move or Line command, followed by x,y coordinates
                    if i + 1 < tokens.len() {
                        i += 1;
                        // Parse coordinate pair (may be "x,y" or separate "x" "y")
                        let coord_str = tokens[i];
                        if let Some((x_str, y_str)) = coord_str.split_once(',') {
                            if let (Ok(x), Ok(y)) = (x_str.parse::<f64>(), y_str.parse::<f64>()) {
                                let adj_x = x - component_data.bbox_x;
                                let adj_y = component_data.bbox_y - y;
                                points.push((adj_x, adj_y));
                            }
                        } else if i + 1 < tokens.len() {
                            // Separate x and y
                            if let (Ok(x), Ok(y)) =
                                (tokens[i].parse::<f64>(), tokens[i + 1].parse::<f64>())
                            {
                                let adj_x = x - component_data.bbox_x;
                                let adj_y = component_data.bbox_y - y;
                                points.push((adj_x, adj_y));
                                i += 1;
                            }
                        }
                    }
                }
                "Z" | "z" => {
                    // Close path: add line from current point back to start point
                    if !points.is_empty() {
                        let first_point = points[0];
                        points.push(first_point);
                    }
                }
                _ => {}
            }
            i += 1;
        }

        if points.len() >= 2 {
            ki_symbol.polylines.push(kicad::KiPolyline {
                points,
                stroke_width: ee_path.stroke_width,
                fill: ee_path.fill,
            });
        }
    }

    // Convert texts with bbox adjustment
    for ee_text in &ee_symbol.texts {
        let adjusted_x = ee_text.x - component_data.bbox_x;
        let adjusted_y = component_data.bbox_y - ee_text.y;

        ki_symbol.texts.push(kicad::SymbolKiText {
            text: ee_text.text.clone(),
            x: adjusted_x,
            y: adjusted_y,
            rotation: ee_text.rotation as f64,
            font_size: ee_text.font_size,
        });
    }

    // Export symbol
    let exporter = kicad::SymbolExporter::new(args.kicad_version());
    let symbol_data = exporter.export(&ki_symbol)?;

    let lib_path = lib_manager.get_symbol_lib_path(args.v5);

    // Use thread-safe add_or_update method
    let status = lib_manager.add_or_update_component(
        &lib_path,
        &ki_symbol.name,
        &symbol_data,
        args.overwrite,
    )?;

    match status {
        SymbolWriteStatus::Added | SymbolWriteStatus::Updated => {
            println!("\u{2713} Symbol converted: {}", ki_symbol.name);
        }
        SymbolWriteStatus::Skipped => {
            println!("Skipped existing symbol: {}", ki_symbol.name);
        }
    }

    Ok(())
}
