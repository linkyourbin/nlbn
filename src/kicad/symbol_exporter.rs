use crate::cli::KicadVersion;
use crate::converter::Converter;
use crate::error::Result;
use crate::kicad::symbol::*;

pub struct SymbolExporter {
    version: KicadVersion,
    converter: Converter,
}

impl SymbolExporter {
    pub fn new(version: KicadVersion) -> Self {
        Self {
            version,
            converter: Converter::new(version),
        }
    }

    pub fn export(&self, symbol: &KiSymbol) -> Result<String> {
        match self.version {
            KicadVersion::V6 => self.export_v6(symbol),
            KicadVersion::V5 => self.export_v5(symbol),
        }
    }

    fn export_v6(&self, symbol: &KiSymbol) -> Result<String> {
        let mut output = String::new();

        // Calculate y_high and y_low from pin positions
        let (y_high, y_low) = self.calculate_y_bounds(symbol);

        // Start symbol definition - match Python formatting
        output.push_str(&format!("  (symbol \"{}\"\n", symbol.name));
        output.push_str("    (in_bom yes)\n");
        output.push_str("    (on_board yes)\n");

        // Properties with proper formatting
        const FIELD_OFFSET_START: f64 = 5.08;
        const FIELD_OFFSET_INCREMENT: f64 = 2.54;
        let mut field_offset_y = FIELD_OFFSET_START;
        let mut property_id = 0;

        // Reference property
        output.push_str("    (property\n");
        output.push_str("      \"Reference\"\n");
        output.push_str(&format!("      \"{}\"\n", symbol.reference));
        output.push_str(&format!("      (id {})\n", property_id));
        output.push_str(&format!("      (at 0 {:.2} 0)\n", y_high + field_offset_y));
        output.push_str("      (effects (font (size 1.27 1.27) ) )\n");
        output.push_str("    )\n");
        property_id += 1;

        // Value property
        output.push_str("    (property\n");
        output.push_str("      \"Value\"\n");
        output.push_str(&format!("      \"{}\"\n", symbol.value));
        output.push_str(&format!("      (id {})\n", property_id));
        output.push_str(&format!("      (at 0 {:.2} 0)\n", y_low - field_offset_y));
        output.push_str("      (effects (font (size 1.27 1.27) ) )\n");
        output.push_str("    )\n");
        property_id += 1;

        // Footprint property
        if !symbol.footprint.is_empty() {
            field_offset_y += FIELD_OFFSET_INCREMENT;
            output.push_str("    (property\n");
            output.push_str("      \"Footprint\"\n");
            output.push_str(&format!("      \"{}\"\n", symbol.footprint));
            output.push_str(&format!("      (id {})\n", property_id));
            output.push_str(&format!("      (at 0 {:.2} 0)\n", y_low - field_offset_y));
            output.push_str("      (effects (font (size 1.27 1.27) ) hide)\n");
            output.push_str("    )\n");
            property_id += 1;
        }

        // Datasheet property
        if !symbol.datasheet.is_empty() {
            field_offset_y += FIELD_OFFSET_INCREMENT;
            output.push_str("    (property\n");
            output.push_str("      \"Datasheet\"\n");
            output.push_str(&format!("      \"{}\"\n", symbol.datasheet));
            output.push_str(&format!("      (id {})\n", property_id));
            output.push_str(&format!("      (at 0 {:.2} 0)\n", y_low - field_offset_y));
            output.push_str("      (effects (font (size 1.27 1.27) ) hide)\n");
            output.push_str("    )\n");
            property_id += 1;
        }

        // Manufacturer property
        if !symbol.manufacturer.is_empty() {
            field_offset_y += FIELD_OFFSET_INCREMENT;
            output.push_str("    (property\n");
            output.push_str("      \"Manufacturer\"\n");
            output.push_str(&format!("      \"{}\"\n", symbol.manufacturer));
            output.push_str(&format!("      (id {})\n", property_id));
            output.push_str(&format!("      (at 0 {:.2} 0)\n", y_low - field_offset_y));
            output.push_str("      (effects (font (size 1.27 1.27) ) hide)\n");
            output.push_str("    )\n");
            property_id += 1;
        }

        // LCSC Part property
        if !symbol.lcsc_id.is_empty() {
            field_offset_y += FIELD_OFFSET_INCREMENT;
            output.push_str("    (property\n");
            output.push_str("      \"LCSC Part\"\n");
            output.push_str(&format!("      \"{}\"\n", symbol.lcsc_id));
            output.push_str(&format!("      (id {})\n", property_id));
            output.push_str(&format!("      (at 0 {:.2} 0)\n", y_low - field_offset_y));
            output.push_str("      (effects (font (size 1.27 1.27) ) hide)\n");
            output.push_str("    )\n");
            property_id += 1;
        }

        // JLC Part property
        if !symbol.jlc_id.is_empty() {
            field_offset_y += FIELD_OFFSET_INCREMENT;
            output.push_str("    (property\n");
            output.push_str("      \"JLC Part\"\n");
            output.push_str(&format!("      \"{}\"\n", symbol.jlc_id));
            output.push_str(&format!("      (id {})\n", property_id));
            output.push_str(&format!("      (at 0 {:.2} 0)\n", y_low - field_offset_y));
            output.push_str("      (effects (font (size 1.27 1.27) ) hide)\n");
            output.push_str("    )\n");
        }

        // Symbol graphics section (unit 0, convert 1) - contains body graphics
        output.push_str(&format!("    (symbol \"{}_0_1\"\n", symbol.name));

        // Rectangles
        for rect in &symbol.rectangles {
            output.push_str(&self.format_rectangle_v6(rect));
        }

        // Circles
        for circle in &symbol.circles {
            output.push_str(&self.format_circle_v6(circle));
        }

        // Arcs
        for arc in &symbol.arcs {
            output.push_str(&self.format_arc_v6(arc));
        }

        // Polylines
        for polyline in &symbol.polylines {
            output.push_str(&self.format_polyline_v6(polyline));
        }

        // Pins - in the same _0_1 section as graphics
        for pin in &symbol.pins {
            output.push_str(&self.format_pin_v6(pin));
        }

        output.push_str("    )\n");
        output.push_str("  )\n");

        Ok(output)
    }

    fn calculate_y_bounds(&self, symbol: &KiSymbol) -> (f64, f64) {
        if symbol.pins.is_empty() {
            return (0.0, 0.0);
        }

        let mut y_high = f64::MIN;
        let mut y_low = f64::MAX;

        for pin in &symbol.pins {
            let y = self.converter.px_to_mm(pin.pos_y);
            if y > y_high {
                y_high = y;
            }
            if y < y_low {
                y_low = y;
            }
        }

        (y_high, y_low)
    }

    fn export_v5(&self, symbol: &KiSymbol) -> Result<String> {
        let mut output = String::new();

        // DEF name reference unused text_offset draw_pinnumber draw_pinname unit_count units_locked option_flag
        output.push_str(&format!(
            "DEF {} {} 0 40 Y Y 1 F N\n",
            symbol.name, symbol.reference
        ));

        // F0 reference x y size orientation visibility hjustify vjustify/italic/bold
        output.push_str(&format!("F0 \"{}\" 0 0 50 H V C CNN\n", symbol.reference));
        output.push_str(&format!("F1 \"{}\" 0 -100 50 H V C CNN\n", symbol.value));
        output.push_str(&format!("F2 \"{}\" 0 0 50 H I C CNN\n", symbol.footprint));
        output.push_str(&format!("F3 \"{}\" 0 0 50 H I C CNN\n", symbol.datasheet));

        // DRAW
        output.push_str("DRAW\n");

        // Rectangles
        for rect in &symbol.rectangles {
            output.push_str(&self.format_rectangle_v5(rect));
        }

        // Circles
        for circle in &symbol.circles {
            output.push_str(&self.format_circle_v5(circle));
        }

        // Polylines
        for polyline in &symbol.polylines {
            output.push_str(&self.format_polyline_v5(polyline));
        }

        // Pins
        for pin in &symbol.pins {
            output.push_str(&self.format_pin_v5(pin));
        }

        output.push_str("ENDDRAW\n");
        output.push_str("ENDDEF\n");

        Ok(output)
    }

    fn format_pin_v6(&self, pin: &KiPin) -> String {
        let x = self.converter.px_to_mm(pin.pos_x);
        let y = self.converter.px_to_mm(pin.pos_y);
        let length = self.converter.px_to_mm(pin.length);

        // Convert pin rotation: (180 + orientation) % 360
        let orientation = (180 + pin.rotation) % 360;

        format!(
            "      (pin {} {}\n        (at {:.2} {:.2} {})\n        (length {:.2})\n        (name \"{}\" (effects (font (size 1.27 1.27))))\n        (number \"{}\" (effects (font (size 1.27 1.27))))\n      )\n",
            pin.pin_type.to_kicad_v6(),
            pin.style.to_kicad_v6(),
            x,
            y,
            orientation,
            length,
            pin.name,
            pin.number
        )
    }

    fn format_pin_v5(&self, pin: &KiPin) -> String {
        let x = self.converter.px_to_mil(pin.pos_x);
        let y = self.converter.px_to_mil(pin.pos_y);  // Don't flip, already handled
        let length = self.converter.px_to_mil(pin.length);

        // X name number posx posy length orientation Snum Snom unit convert Etype [shape]
        format!(
            "X {} {} {} {} {} {} {} {} {} {} {}\n",
            pin.name,
            pin.number,
            x,
            y,
            length,
            self.rotation_to_direction(pin.rotation),
            50, // name size
            50, // number size
            1,  // unit
            1,  // convert
            pin.pin_type.to_kicad_v5()
        )
    }

    fn format_rectangle_v6(&self, rect: &KiRectangle) -> String {
        let x1 = self.converter.px_to_mm(rect.x1);
        let y1 = self.converter.px_to_mm(rect.y1);
        let x2 = self.converter.px_to_mm(rect.x2);
        let y2 = self.converter.px_to_mm(rect.y2);
        let _width = self.converter.px_to_mm(rect.stroke_width);

        let fill = if rect.fill { "background" } else { "none" };

        format!(
            "      (rectangle\n        (start {:.2} {:.2})\n        (end {:.2} {:.2})\n        (stroke (width {}) (type default) (color 0 0 0 0))\n        (fill (type {}))\n      )\n",
            x1, y1, x2, y2, 0, fill
        )
    }

    fn format_rectangle_v5(&self, rect: &KiRectangle) -> String {
        let x1 = self.converter.px_to_mil(rect.x1);
        let y1 = self.converter.px_to_mil(rect.y1);  // Don't flip, already handled
        let x2 = self.converter.px_to_mil(rect.x2);
        let y2 = self.converter.px_to_mil(rect.y2);  // Don't flip, already handled

        let fill = if rect.fill { "F" } else { "N" };

        // S startx starty endx endy unit convert thickness fill
        format!("S {} {} {} {} 1 1 10 {}\n", x1, y1, x2, y2, fill)
    }

    fn format_circle_v6(&self, circle: &KiCircle) -> String {
        let cx = self.converter.px_to_mm(circle.cx);
        let cy = self.converter.px_to_mm(circle.cy);
        let radius = self.converter.px_to_mm(circle.radius);

        // Circles in symbols should always have fill type "none" to match Python output
        let fill = "none";

        format!(
            "      (circle\n        (center {:.2} {:.2})\n        (radius {:.2})\n        (stroke (width {}) (type default) (color 0 0 0 0))\n        (fill (type {}))\n      )\n",
            cx, cy, radius, 0, fill
        )
    }

    fn format_circle_v5(&self, circle: &KiCircle) -> String {
        let cx = self.converter.px_to_mil(circle.cx);
        let cy = self.converter.px_to_mil(circle.cy);  // Don't flip, already handled
        let radius = self.converter.px_to_mil(circle.radius);

        let fill = if circle.fill { "F" } else { "N" };

        // C posx posy radius unit convert thickness fill
        format!("C {} {} {} 1 1 10 {}\n", cx, cy, radius, fill)
    }

    fn format_arc_v6(&self, arc: &KiArc) -> String {
        let start_x = self.converter.px_to_mm(arc.start_x);
        let start_y = self.converter.px_to_mm(arc.start_y);  // Don't flip, already handled
        let mid_x = self.converter.px_to_mm(arc.mid_x);
        let mid_y = self.converter.px_to_mm(arc.mid_y);  // Don't flip, already handled
        let end_x = self.converter.px_to_mm(arc.end_x);
        let end_y = self.converter.px_to_mm(arc.end_y);  // Don't flip, already handled
        let width = self.converter.px_to_mm(arc.stroke_width);

        format!(
            "    (arc (start {:.4} {:.4}) (mid {:.4} {:.4}) (end {:.4} {:.4})\n      (stroke (width {:.4}) (type default))\n      (fill (type none))\n    )\n",
            start_x, start_y, mid_x, mid_y, end_x, end_y, width
        )
    }

    fn format_polyline_v6(&self, polyline: &KiPolyline) -> String {
        let mut output = String::from("    (polyline\n      (pts\n");

        for (x, y) in &polyline.points {
            let x = self.converter.px_to_mm(*x);
            let y = self.converter.px_to_mm(*y);  // Don't flip, already handled
            output.push_str(&format!("        (xy {:.4} {:.4})\n", x, y));
        }

        let width = self.converter.px_to_mm(polyline.stroke_width);
        let fill = if polyline.fill { "background" } else { "none" };

        output.push_str("      )\n");
        output.push_str(&format!("      (stroke (width {:.4}) (type default))\n", width));
        output.push_str(&format!("      (fill (type {}))\n", fill));
        output.push_str("    )\n");

        output
    }

    fn format_polyline_v5(&self, polyline: &KiPolyline) -> String {
        let point_count = polyline.points.len();
        let mut output = format!("P {} 1 1 10", point_count);

        for (x, y) in &polyline.points {
            let x = self.converter.px_to_mil(*x);
            let y = self.converter.px_to_mil(*y);  // Don't flip, already handled
            output.push_str(&format!(" {} {}", x, y));
        }

        let fill = if polyline.fill { "F" } else { "N" };
        output.push_str(&format!(" {}\n", fill));

        output
    }

    fn rotation_to_direction(&self, rotation: i32) -> char {
        match rotation {
            0 => 'R',
            90 => 'U',
            180 => 'L',
            270 => 'D',
            _ => 'R',
        }
    }
}
