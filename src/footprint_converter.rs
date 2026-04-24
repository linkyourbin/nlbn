use crate::cli::Cli;
use crate::converter::{Converter, angle_to_ki, sanitize_name};
use crate::easyeda::svg_parser::{SvgCommand, parse_svg_path};
use crate::easyeda::{ComponentData, FootprintImporter};
use crate::error::Result;
use crate::kicad;
use crate::library::{FileWriteStatus, LibraryManager};

pub fn convert_footprint(
    args: &Cli,
    component_data: &ComponentData,
    lib_manager: &LibraryManager,
    lcsc_id: &str,
) -> Result<()> {
    let ee_footprint = FootprintImporter::parse(&component_data.package_detail)?;
    let converter = Converter::new(args.kicad_version());

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
        let adjusted_x_mm = converter.px_to_mm(adjusted_x);
        let adjusted_y_mm = converter.px_to_mm(adjusted_y);

        // Handle polygon pads
        let (size_x, size_y, rotation, polygon) =
            if ee_pad.shape == "POLYGON" && !ee_pad.points.is_empty() {
                // Parse points: space-separated x y coordinates
                let coords: Vec<f64> = ee_pad
                    .points
                    .split_whitespace()
                    .filter_map(|s| s.parse::<f64>().ok())
                    .collect();

                if coords.len() >= 4 {
                    // At least 2 points (x,y pairs)
                    // Convert coordinates to mm and make relative to pad position
                    let mut poly_str =
                        String::from("\n\t\t(primitives \n\t\t\t(gr_poly \n\t\t\t\t(pts");

                    for i in (0..coords.len()).step_by(2) {
                        if i + 1 < coords.len() {
                            let abs_x_mm =
                                converter.px_to_mm(coords[i] - component_data.package_bbox_x);
                            let abs_y_mm =
                                converter.px_to_mm(coords[i + 1] - component_data.package_bbox_y);
                            let rel_x = abs_x_mm - adjusted_x_mm;
                            let rel_y = abs_y_mm - adjusted_y_mm;
                            poly_str.push_str(&format!(" (xy {:.2} {:.2})", rel_x, rel_y));
                        }
                    }

                    poly_str.push_str("\n\t\t\t\t) \n\t\t\t\t(width 0.1) \n\t\t\t)\n\t\t)\n\t");

                    // Set minimal pad size (enforced minimum 0.01) and force orientation to 0
                    (0.01, 0.01, 0.0, Some(poly_str))
                } else {
                    let rot = angle_to_ki(ee_pad.rotation);
                    (ee_pad.width.max(0.01), ee_pad.height.max(0.01), rot, None)
                }
            } else {
                let rot = angle_to_ki(ee_pad.rotation);
                (ee_pad.width.max(0.01), ee_pad.height.max(0.01), rot, None)
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
        let coords: Vec<f64> = ee_track
            .points
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
            width: converter.px_to_mm(ee_circle.stroke_width).max(0.01),
            layer: kicad::map_layer(ee_circle.layer_id),
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
            number: String::new(), // Empty number for non-plated holes
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
            number: String::new(), // Vias typically don't have pad numbers
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

    // Convert arcs with bbox adjustment (SVG path format)
    for ee_arc in &ee_footprint.arcs {
        let commands = match parse_svg_path(&ee_arc.path) {
            Ok(cmds) => cmds,
            Err(e) => {
                log::warn!(
                    "Skipping arc with invalid SVG path: {} ({})",
                    ee_arc.path,
                    e
                );
                continue;
            }
        };

        let mut current_pos = (0.0_f64, 0.0_f64);
        for cmd in &commands {
            match cmd {
                SvgCommand::MoveTo { x, y } => {
                    current_pos = (*x, *y);
                }
                SvgCommand::Arc {
                    rx,
                    ry,
                    angle,
                    large_arc,
                    sweep,
                    x,
                    y,
                } => {
                    let start_x = current_pos.0;
                    let start_y = current_pos.1;
                    let end_x = *x;
                    let end_y = *y;

                    match converter.compute_arc_center(
                        (start_x, start_y),
                        (end_x, end_y),
                        (*rx, *ry),
                        *angle,
                        *large_arc,
                        *sweep,
                    ) {
                        Ok((cx, cy, start_angle_deg, end_angle_deg)) => {
                            let start_rad = start_angle_deg.to_radians();
                            let end_rad = end_angle_deg.to_radians();
                            let mut mid_angle = (start_rad + end_rad) / 2.0;

                            let angle_diff = end_rad - start_rad;
                            if (*sweep && angle_diff < 0.0) || (!*sweep && angle_diff > 0.0) {
                                mid_angle += std::f64::consts::PI;
                            }

                            let mid_x = cx + rx * mid_angle.cos();
                            let mid_y = cy + ry * mid_angle.sin();

                            let adj_start_x = start_x - component_data.package_bbox_x;
                            let adj_start_y = start_y - component_data.package_bbox_y;
                            let adj_mid_x = mid_x - component_data.package_bbox_x;
                            let adj_mid_y = mid_y - component_data.package_bbox_y;
                            let adj_end_x = end_x - component_data.package_bbox_x;
                            let adj_end_y = end_y - component_data.package_bbox_y;

                            ki_footprint.arcs.push(kicad::FootprintKiArc {
                                start_x: adj_start_x,
                                start_y: adj_start_y,
                                mid_x: adj_mid_x,
                                mid_y: adj_mid_y,
                                end_x: adj_end_x,
                                end_y: adj_end_y,
                                width: ee_arc.stroke_width,
                                layer: kicad::map_layer(ee_arc.layer_id),
                            });
                        }
                        Err(e) => {
                            log::warn!("Failed to compute arc center: {}", e);
                        }
                    }

                    current_pos = (end_x, end_y);
                }
                SvgCommand::LineTo { x, y } => {
                    current_pos = (*x, *y);
                }
                SvgCommand::ClosePath => {}
            }
        }
    }

    // Convert rectangles to 4 lines
    for ee_rect in &ee_footprint.rectangles {
        let adjusted_x = ee_rect.x - component_data.package_bbox_x;
        let adjusted_y = ee_rect.y - component_data.package_bbox_y;
        let adjusted_x2 = (ee_rect.x + ee_rect.width) - component_data.package_bbox_x;
        let adjusted_y2 = (ee_rect.y + ee_rect.height) - component_data.package_bbox_y;

        let layer = kicad::map_layer(ee_rect.layer_id);
        let width = converter.px_to_mm(ee_rect.stroke_width).max(0.01);

        // Top line
        ki_footprint.lines.push(kicad::KiLine {
            start_x: adjusted_x,
            start_y: adjusted_y,
            end_x: adjusted_x2,
            end_y: adjusted_y,
            width,
            layer: layer.clone(),
        });

        // Right line
        ki_footprint.lines.push(kicad::KiLine {
            start_x: adjusted_x2,
            start_y: adjusted_y,
            end_x: adjusted_x2,
            end_y: adjusted_y2,
            width,
            layer: layer.clone(),
        });

        // Bottom line
        ki_footprint.lines.push(kicad::KiLine {
            start_x: adjusted_x2,
            start_y: adjusted_y2,
            end_x: adjusted_x,
            end_y: adjusted_y2,
            width,
            layer: layer.clone(),
        });

        // Left line
        ki_footprint.lines.push(kicad::KiLine {
            start_x: adjusted_x,
            start_y: adjusted_y2,
            end_x: adjusted_x,
            end_y: adjusted_y,
            width,
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
            layer: kicad::map_layer(ee_text.layer_id),
            size: ee_text.font_size,
            thickness: converter.px_to_mm(ee_text.stroke_width).max(0.01),
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
            let model_lib_name = lib_manager.model_lib_name();
            let model_dir_name = lib_manager.model_dir_name();
            let model_path = if args.project_relative {
                format!("${{KIPRJMOD}}/{}/{}.step", model_dir_name, model_name)
            } else {
                format!(
                    "${{{}}}/{}/{}.step",
                    model_lib_name, model_dir_name, model_name
                )
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
    let exporter = kicad::FootprintExporter::new();
    let footprint_data = exporter.export(&ki_footprint)?;
    let (_, status) = lib_manager.write_footprint_if_needed(
        &ki_footprint.name,
        &footprint_data,
        args.overwrite,
    )?;

    match status {
        FileWriteStatus::Written => {
            println!("\u{2713} Footprint converted: {}", ki_footprint.name);
        }
        FileWriteStatus::Skipped => {
            println!("Skipped existing footprint: {}", ki_footprint.name);
        }
    }

    Ok(())
}
