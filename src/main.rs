use clap::Parser;
use nlbn::*;
use std::process;
use std::sync::{Arc, Mutex};
use rayon::prelude::*;

fn main() {
    // Initialize logger with custom format to hide module paths
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .format(|buf, record| {
            use std::io::Write;
            writeln!(
                buf,
                "[{} {} nlbn] {}",
                chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%.3fZ"),
                record.level(),
                record.args()
            )
        })
        .init();

    // Parse CLI arguments
    let args = Cli::parse();

    // Set debug logging if requested
    if args.debug {
        log::set_max_level(log::LevelFilter::Debug);
    }

    // Run the conversion
    if let Err(e) = run(args) {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}

fn run(args: Cli) -> error::Result<()> {
    // Validate arguments
    args.validate()?;

    // Get list of LCSC IDs to process
    let lcsc_ids = args.get_lcsc_ids()?;
    let total_count = lcsc_ids.len();
    let is_batch = total_count > 1;

    if is_batch {
        log::info!("Batch mode: processing {} components", total_count);
        if args.parallel > 1 {
            log::info!("Parallel downloads: {} threads", args.parallel);
        }
    }

    // Setup output directories
    let lib_manager = LibraryManager::new(&args.output);
    lib_manager.create_directories()?;

    // Initialize API
    let api = EasyedaApi::new();

    // Track statistics
    let success_count = Arc::new(Mutex::new(0));
    let failed_count = Arc::new(Mutex::new(0));
    let failed_ids = Arc::new(Mutex::new(Vec::new()));

    if is_batch && args.parallel > 1 {
        // Parallel processing mode
        rayon::ThreadPoolBuilder::new()
            .num_threads(args.parallel)
            .build()
            .unwrap()
            .install(|| {
                lcsc_ids.par_iter().enumerate().for_each(|(index, lcsc_id)| {
                    println!("\n[{}/{}] Processing: {}", index + 1, total_count, lcsc_id);

                    // Process single component
                    match process_component(&args, &api, &lib_manager, lcsc_id) {
                        Ok(_) => {
                            *success_count.lock().unwrap() += 1;
                            println!("✓ [{}/{}] Success: {}", index + 1, total_count, lcsc_id);
                        }
                        Err(e) => {
                            *failed_count.lock().unwrap() += 1;
                            failed_ids.lock().unwrap().push(lcsc_id.clone());

                            if args.continue_on_error {
                                eprintln!("✗ [{}/{}] Failed: {} - {}", index + 1, total_count, lcsc_id, e);
                                log::error!("Failed to process {}: {}", lcsc_id, e);
                            }
                        }
                    }
                });
            });
    } else {
        // Sequential processing mode
        for (index, lcsc_id) in lcsc_ids.iter().enumerate() {
            if is_batch {
                println!("\n[{}/{}] Processing: {}", index + 1, total_count, lcsc_id);
            } else {
                log::info!("Starting conversion for LCSC ID: {}", lcsc_id);
            }

            // Process single component
            match process_component(&args, &api, &lib_manager, lcsc_id) {
                Ok(_) => {
                    *success_count.lock().unwrap() += 1;
                    if is_batch {
                        println!("✓ Success: {}", lcsc_id);
                    }
                }
                Err(e) => {
                    *failed_count.lock().unwrap() += 1;
                    failed_ids.lock().unwrap().push(lcsc_id.clone());

                    if args.continue_on_error {
                        eprintln!("✗ Failed: {} - {}", lcsc_id, e);
                        log::error!("Failed to process {}: {}", lcsc_id, e);
                    } else {
                        return Err(e);
                    }
                }
            }
        }
    }

    let success = *success_count.lock().unwrap();
    let failed = *failed_count.lock().unwrap();
    let failed_list = failed_ids.lock().unwrap().clone();

    // Print summary for batch mode
    if is_batch {
        println!("\n{}", "=".repeat(60));
        println!("Batch conversion complete!");
        println!("Total: {} | Success: {} | Failed: {}", total_count, success, failed);

        if !failed_list.is_empty() {
            println!("\nFailed components:");
            for id in &failed_list {
                println!("  - {}", id);
            }
        }

        println!("Output directory: {}", args.output.display());
        println!("{}", "=".repeat(60));
    } else {
        println!("\n✓ Conversion complete!");
        println!("Output directory: {}", args.output.display());
    }

    Ok(())
}

fn process_component(args: &Cli, api: &EasyedaApi, lib_manager: &LibraryManager, lcsc_id: &str) -> error::Result<()> {
    // Fetch component data from EasyEDA API
    let component_data = api.get_component_data(lcsc_id)?;

    log::info!("Fetched component: {}", component_data.title);

    // Process symbol (if requested)
    if args.symbol || args.full {
        log::info!("Converting symbol...");

        let ee_symbol = SymbolImporter::parse(&component_data.data_str)?;

        // Use LCSC ID as unique identifier to prevent name collisions
        let component_name = format!("{}_{}", sanitize_name(&component_data.title), lcsc_id);
        let footprint_name = component_name.clone();

        let mut ki_symbol = kicad::KiSymbol {
            name: component_name.clone(),
            reference: ee_symbol.prefix.clone(),
            value: component_data.title.clone(),
            footprint: format!("nlbn:{}", footprint_name),
            datasheet: component_data.datasheet.clone(),
            manufacturer: component_data.manufacturer.clone(),
            lcsc_id: component_data.lcsc_id.clone(),
            jlc_id: component_data.jlc_id.clone(),
            pins: Vec::new(),
            rectangles: Vec::new(),
            circles: Vec::new(),
            arcs: Vec::new(),
            polylines: Vec::new(),
        };

        // Convert pins with bbox adjustment
        let _converter = Converter::new(args.kicad_version());

        log::debug!("bbox_x = {}, bbox_y = {}", component_data.bbox_x, component_data.bbox_y);

        for ee_pin in &ee_symbol.pins {
            let adjusted_x = ee_pin.x - component_data.bbox_x;
            let adjusted_y = ee_pin.y - component_data.bbox_y;

            if ee_pin.name.contains("PG10") {
                log::info!("PG10 pin: raw x={}, y={}, adjusted x={}, y={}, final y={}",
                    ee_pin.x, ee_pin.y, adjusted_x, adjusted_y, -adjusted_y);
            }

            // Log pins with unusual length
            if ee_pin.length >= 100.0 {
                log::warn!("Pin {} ({}) has unusual length: {}", ee_pin.number, ee_pin.name, ee_pin.length);
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
                pos_y: -adjusted_y,  // Back to negation to test
                rotation: ee_pin.rotation,
                length: ee_pin.length,
            });
        }

        // Convert rectangles with bbox adjustment
        for (idx, ee_rect) in ee_symbol.rectangles.iter().enumerate() {
            let adjusted_x = ee_rect.x - component_data.bbox_x;
            let adjusted_y = component_data.bbox_y - ee_rect.y;  // bbox_y - pos_y
            let adjusted_x2 = (ee_rect.x + ee_rect.width) - component_data.bbox_x;
            let adjusted_y2 = component_data.bbox_y - (ee_rect.y + ee_rect.height);  // bbox_y - (pos_y + height)

            // First rectangle is usually the main body, should be filled
            let fill = if idx == 0 { true } else { ee_rect.fill };

            ki_symbol.rectangles.push(kicad::KiRectangle {
                x1: adjusted_x,
                y1: adjusted_y,  // No negation
                x2: adjusted_x2,
                y2: adjusted_y2,  // No negation
                stroke_width: ee_rect.stroke_width,
                fill,
            });
        }

        // Convert circles with bbox adjustment
        for ee_circle in &ee_symbol.circles {
            let adjusted_cx = ee_circle.cx - component_data.bbox_x;
            let adjusted_cy = component_data.bbox_y - ee_circle.cy;  // bbox_y - pos_y

            ki_symbol.circles.push(kicad::KiCircle {
                cx: adjusted_cx,
                cy: adjusted_cy,  // No negation
                radius: ee_circle.radius,
                stroke_width: ee_circle.stroke_width,
                fill: ee_circle.fill,
            });
        }

        // Convert ellipses with bbox adjustment
        // If rx == ry, treat as circle; otherwise, approximate as circle with average radius
        for ee_ellipse in &ee_symbol.ellipses {
            let adjusted_cx = ee_ellipse.cx - component_data.bbox_x;
            let adjusted_cy = component_data.bbox_y - ee_ellipse.cy;  // bbox_y - pos_y

            // Use average of rx and ry as radius (or just rx if they're equal)
            let radius = (ee_ellipse.rx + ee_ellipse.ry) / 2.0;

            ki_symbol.circles.push(kicad::KiCircle {
                cx: adjusted_cx,
                cy: adjusted_cy,  // No negation
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
            let adjusted_points: Vec<(f64, f64)> = ee_polyline.points.iter()
                .map(|(x, y)| {
                    let adj_x = x - component_data.bbox_x;
                    let adj_y = component_data.bbox_y - y;  // bbox_y - pos_y
                    (adj_x, adj_y)  // No negation
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
            let adjusted_points: Vec<(f64, f64)> = ee_polygon.points.iter()
                .map(|(x, y)| {
                    let adj_x = x - component_data.bbox_x;
                    let adj_y = component_data.bbox_y - y;  // bbox_y - pos_y
                    (adj_x, adj_y)  // No negation
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
                                if let (Ok(x), Ok(y)) = (tokens[i].parse::<f64>(), tokens[i + 1].parse::<f64>()) {
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

        // Export symbol
        let exporter = SymbolExporter::new(args.kicad_version());
        let symbol_data = exporter.export(&ki_symbol)?;

        let lib_path = lib_manager.get_symbol_lib_path(args.v5);

        // Use thread-safe add_or_update method
        lib_manager.add_or_update_component(&lib_path, &ki_symbol.name, &symbol_data, args.overwrite)?;

        println!("✓ Symbol converted: {}", ki_symbol.name);
    }

    // Process footprint (if requested)
    if args.footprint || args.full {
        log::info!("Converting footprint...");

        let ee_footprint = FootprintImporter::parse(&component_data.package_detail)?;
        let _converter = Converter::new(args.kicad_version());

        // Use LCSC ID as unique identifier to prevent name collisions
        let footprint_name = format!("{}_{}", sanitize_name(&component_data.title), lcsc_id);

        // Convert EasyEDA footprint to KiCad footprint
        let mut ki_footprint = kicad::KiFootprint {
            name: footprint_name,
            pads: Vec::new(),
            tracks: Vec::new(),
            circles: Vec::new(),
            arcs: Vec::new(),
            texts: Vec::new(),
            lines: Vec::new(),
            model_3d: None,
        };

        // Convert pads with bbox adjustment
        for ee_pad in &ee_footprint.pads {
            let pad_type = if ee_pad.hole_radius.is_some() {
                kicad::PadType::ThroughHole
            } else {
                kicad::PadType::Smd
            };

            // Use layer mapping based on pad type
            let layers = if pad_type == kicad::PadType::ThroughHole {
                kicad::map_pad_layers_tht(ee_pad.layer_id)
            } else {
                kicad::map_pad_layers_smd(ee_pad.layer_id)
            };

            // Create drill for through-hole pads
            let drill = if let Some(hole_radius) = ee_pad.hole_radius {
                if let Some(hole_length) = ee_pad.hole_length {
                    // Elliptical drill
                    let max_distance_hole = (hole_radius * 2.0).max(hole_length);
                    let pos_0 = ee_pad.height - max_distance_hole;
                    let pos_90 = ee_pad.width - max_distance_hole;

                    if pos_0 > pos_90 {
                        // Vertical orientation
                        Some(kicad::Drill {
                            diameter: hole_radius * 2.0,
                            width: Some(hole_length),
                            offset_x: 0.0,
                            offset_y: 0.0,
                        })
                    } else {
                        // Horizontal orientation
                        Some(kicad::Drill {
                            diameter: hole_length,
                            width: Some(hole_radius * 2.0),
                            offset_x: 0.0,
                            offset_y: 0.0,
                        })
                    }
                } else {
                    // Circular drill
                    Some(kicad::Drill {
                        diameter: hole_radius * 2.0,
                        width: None,
                        offset_x: 0.0,
                        offset_y: 0.0,
                    })
                }
            } else {
                None
            };

            // Apply bbox normalization for footprint coordinates
            let adjusted_x = ee_pad.x - component_data.package_bbox_x;
            let adjusted_y = ee_pad.y - component_data.package_bbox_y;

            // Handle polygon pads
            let (size_x, size_y, rotation, polygon) = if ee_pad.shape == "POLYGON" && !ee_pad.points.is_empty() {
                // Parse points: space-separated x y coordinates
                let coords: Vec<f64> = ee_pad.points
                    .split_whitespace()
                    .filter_map(|s| s.parse::<f64>().ok())
                    .collect();

                if coords.len() >= 4 {  // At least 2 points (x,y pairs)
                    // Generate polygon with coordinates relative to pad position
                    let mut poly_str = String::from("\n\t\t(primitives \n\t\t\t(gr_poly \n\t\t\t\t(pts");

                    for i in (0..coords.len()).step_by(2) {
                        if i + 1 < coords.len() {
                            let rel_x = coords[i] - component_data.package_bbox_x - adjusted_x;
                            let rel_y = coords[i + 1] - component_data.package_bbox_y - adjusted_y;
                            poly_str.push_str(&format!(" (xy {:.2} {:.2})", rel_x, rel_y));
                        }
                    }

                    poly_str.push_str("\n\t\t\t\t) \n\t\t\t\t(width 0.1) \n\t\t\t)\n\t\t)\n\t");

                    // Set minimal pad size and force orientation to 0
                    (0.005, 0.005, 0.0, Some(poly_str))
                } else {
                    (ee_pad.width, ee_pad.height, ee_pad.rotation, None)
                }
            } else {
                (ee_pad.width, ee_pad.height, ee_pad.rotation, None)
            };

            ki_footprint.pads.push(kicad::KiPad {
                number: ee_pad.number.clone(),
                pad_type,
                shape: kicad::PadShape::from_easyeda(&ee_pad.shape),
                pos_x: adjusted_x,
                pos_y: adjusted_y,
                size_x,
                size_y,
                rotation,
                layers,
                drill,
                polygon,
            });
        }

        // Convert tracks to lines with bbox adjustment
        // TRACK has a points string: "x1 y1 x2 y2 x3 y3..." which represents a polyline
        // We need to convert it to multiple line segments
        for ee_track in &ee_footprint.tracks {
            // Parse points string into coordinates
            let coords: Vec<f64> = ee_track.points
                .split_whitespace()
                .filter_map(|s| s.parse::<f64>().ok())
                .collect();

            // Create line segments from consecutive point pairs
            // Each pair of points (x1,y1) -> (x2,y2) becomes one line
            for i in (0..coords.len().saturating_sub(2)).step_by(2) {
                if i + 3 < coords.len() {
                    let x1 = coords[i];
                    let y1 = coords[i + 1];
                    let x2 = coords[i + 2];
                    let y2 = coords[i + 3];

                    let adjusted_x1 = x1 - component_data.package_bbox_x;
                    let adjusted_y1 = y1 - component_data.package_bbox_y;
                    let adjusted_x2 = x2 - component_data.package_bbox_x;
                    let adjusted_y2 = y2 - component_data.package_bbox_y;

                    ki_footprint.lines.push(kicad::KiLine {
                        start_x: adjusted_x1,
                        start_y: adjusted_y1,
                        end_x: adjusted_x2,
                        end_y: adjusted_y2,
                        width: ee_track.stroke_width,
                        layer: kicad::map_layer(ee_track.layer_id),
                    });
                }
            }
        }

        // Convert circles with bbox adjustment
        for ee_circle in &ee_footprint.circles {
            let adjusted_cx = ee_circle.cx - component_data.package_bbox_x;
            let adjusted_cy = ee_circle.cy - component_data.package_bbox_y;

            ki_footprint.circles.push(kicad::FootprintKiCircle {
                center_x: adjusted_cx,
                center_y: adjusted_cy,
                end_x: adjusted_cx + ee_circle.radius,
                end_y: adjusted_cy,
                width: ee_circle.stroke_width,
                layer: "F.SilkS".to_string(),
                fill: ee_circle.fill,
            });
        }

        // Convert holes to non-plated through-hole pads
        for ee_hole in &ee_footprint.holes {
            let adjusted_x = ee_hole.x - component_data.package_bbox_x;
            let adjusted_y = ee_hole.y - component_data.package_bbox_y;

            // EasyEDA stores radius, so diameter = radius * 2
            let diameter = ee_hole.radius * 2.0;

            ki_footprint.pads.push(kicad::KiPad {
                number: String::new(),  // Empty number for non-plated holes
                pad_type: kicad::PadType::NpThroughHole,
                shape: kicad::PadShape::Circle,
                pos_x: adjusted_x,
                pos_y: adjusted_y,
                size_x: diameter,
                size_y: diameter,
                rotation: 0.0,
                layers: vec!["*.Cu".to_string(), "*.Mask".to_string()],
                drill: Some(kicad::Drill {
                    diameter,
                    width: None,
                    offset_x: 0.0,
                    offset_y: 0.0,
                }),
                polygon: None,
            });
        }

        // Convert vias to through-hole pads
        for ee_via in &ee_footprint.vias {
            let adjusted_x = ee_via.x - component_data.package_bbox_x;
            let adjusted_y = ee_via.y - component_data.package_bbox_y;

            // Via has diameter (pad size) and radius (hole radius, so drill = radius * 2)
            let pad_size = ee_via.diameter;
            let drill_diameter = ee_via.radius * 2.0;

            ki_footprint.pads.push(kicad::KiPad {
                number: String::new(),  // Vias typically don't have pad numbers
                pad_type: kicad::PadType::ThroughHole,
                shape: kicad::PadShape::Circle,
                pos_x: adjusted_x,
                pos_y: adjusted_y,
                size_x: pad_size,
                size_y: pad_size,
                rotation: 0.0,
                layers: vec!["*.Cu".to_string(), "*.Mask".to_string()],
                drill: Some(kicad::Drill {
                    diameter: drill_diameter,
                    width: None,
                    offset_x: 0.0,
                    offset_y: 0.0,
                }),
                polygon: None,
            });
        }

        // Convert arcs with bbox adjustment
        for ee_arc in &ee_footprint.arcs {
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
            let adjusted_start_x = start_x - component_data.package_bbox_x;
            let adjusted_start_y = start_y - component_data.package_bbox_y;
            let adjusted_mid_x = mid_x - component_data.package_bbox_x;
            let adjusted_mid_y = mid_y - component_data.package_bbox_y;
            let adjusted_end_x = end_x - component_data.package_bbox_x;
            let adjusted_end_y = end_y - component_data.package_bbox_y;

            ki_footprint.arcs.push(kicad::FootprintKiArc {
                start_x: adjusted_start_x,
                start_y: adjusted_start_y,
                mid_x: adjusted_mid_x,
                mid_y: adjusted_mid_y,
                end_x: adjusted_end_x,
                end_y: adjusted_end_y,
                width: ee_arc.stroke_width,
                layer: "F.SilkS".to_string(),
            });
        }

        // Convert rectangles to 4 lines
        for ee_rect in &ee_footprint.rectangles {
            let adjusted_x = ee_rect.x - component_data.package_bbox_x;
            let adjusted_y = ee_rect.y - component_data.package_bbox_y;
            let adjusted_x2 = (ee_rect.x + ee_rect.width) - component_data.package_bbox_x;
            let adjusted_y2 = (ee_rect.y + ee_rect.height) - component_data.package_bbox_y;

            let layer = "F.SilkS".to_string();

            // Top line
            ki_footprint.lines.push(kicad::KiLine {
                start_x: adjusted_x,
                start_y: adjusted_y,
                end_x: adjusted_x2,
                end_y: adjusted_y,
                width: ee_rect.stroke_width,
                layer: layer.clone(),
            });

            // Right line
            ki_footprint.lines.push(kicad::KiLine {
                start_x: adjusted_x2,
                start_y: adjusted_y,
                end_x: adjusted_x2,
                end_y: adjusted_y2,
                width: ee_rect.stroke_width,
                layer: layer.clone(),
            });

            // Bottom line
            ki_footprint.lines.push(kicad::KiLine {
                start_x: adjusted_x2,
                start_y: adjusted_y2,
                end_x: adjusted_x,
                end_y: adjusted_y2,
                width: ee_rect.stroke_width,
                layer: layer.clone(),
            });

            // Left line
            ki_footprint.lines.push(kicad::KiLine {
                start_x: adjusted_x,
                start_y: adjusted_y2,
                end_x: adjusted_x,
                end_y: adjusted_y,
                width: ee_rect.stroke_width,
                layer,
            });
        }

        // Convert texts with bbox adjustment
        for ee_text in &ee_footprint.texts {
            let adjusted_x = ee_text.x - component_data.package_bbox_x;
            let adjusted_y = ee_text.y - component_data.package_bbox_y;

            ki_footprint.texts.push(kicad::KiText {
                text: ee_text.text.clone(),
                pos_x: adjusted_x,
                pos_y: adjusted_y,
                rotation: ee_text.rotation as f64,
                layer: "F.SilkS".to_string(),
                size: ee_text.font_size,
                thickness: 0.15,
            });
        }

        // Add 3D model reference if available
        if let Some(model_info) = &component_data.model_3d {
            if args.model_3d || args.full {
                // Use LCSC ID as unique identifier to prevent name collisions
                let model_name = format!("{}_{}", sanitize_name(&model_info.title), lcsc_id);

                // Default to project-relative paths (KIPRJMOD) for easier setup
                // Use --project-relative flag to force global paths if needed
                // Prefer STEP format as it's more widely supported
                let model_path = if args.project_relative {
                    format!("${{KIPRJMOD}}/nlbn.3dshapes/{}.step", model_name)
                } else {
                    format!("${{NLBN}}/nlbn.3dshapes/{}.step", model_name)
                };

                ki_footprint.model_3d = Some(kicad::Ki3dModel {
                    path: model_path,
                    offset: (0.0, 0.0, 0.0),
                    scale: (1.0, 1.0, 1.0),
                    rotate: (0.0, 0.0, 0.0),
                });
            }
        }

        // Export footprint
        let exporter = FootprintExporter::new();
        let footprint_data = exporter.export(&ki_footprint)?;
        lib_manager.write_footprint(&ki_footprint.name, &footprint_data)?;

        println!("✓ Footprint converted: {}", ki_footprint.name);
    }

    // Process 3D model (if requested)
    if args.model_3d || args.full {
        if let Some(model_info) = &component_data.model_3d {
            log::info!("Converting 3D model...");

            // Use LCSC ID as unique identifier to prevent name collisions
            let model_name = format!("{}_{}", sanitize_name(&model_info.title), lcsc_id);
            let exporter = ModelExporter::new();

            let mut has_model = false;

            // Download and convert OBJ to WRL
            match api.download_3d_obj(&model_info.uuid) {
                Ok(obj_data) => {
                    match exporter.obj_to_wrl(&obj_data) {
                        Ok(wrl_data) => {
                            match lib_manager.write_wrl_model(&model_name, &wrl_data) {
                                Ok(_) => {
                                    log::info!("✓ WRL model converted: {}", model_name);
                                    has_model = true;
                                }
                                Err(e) => log::warn!("Failed to write WRL model: {}", e),
                            }
                        }
                        Err(e) => log::warn!("Failed to convert OBJ to WRL: {}", e),
                    }
                }
                Err(e) => log::warn!("Failed to download OBJ model: {}", e),
            }

            // Download STEP format
            match api.download_3d_step(&model_info.uuid) {
                Ok(step_data) => {
                    match exporter.export_step(&step_data) {
                        Ok(step_data) => {
                            match lib_manager.write_step_model(&model_name, &step_data) {
                                Ok(_) => {
                                    log::info!("✓ STEP model converted: {}", model_name);
                                    has_model = true;
                                }
                                Err(e) => log::warn!("Failed to write STEP model: {}", e),
                            }
                        }
                        Err(e) => log::warn!("Failed to export STEP model: {}", e),
                    }
                }
                Err(e) => log::warn!("Failed to download STEP model: {}", e),
            }

            if has_model {
                println!("✓ 3D model converted: {} (WRL + STEP)", model_name);
            } else {
                println!("⚠ 3D model not available");
            }
        } else {
            log::warn!("No 3D model metadata available for this component");
        }
    }

    Ok(())
}

fn sanitize_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}
