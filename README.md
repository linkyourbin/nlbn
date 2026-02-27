<div align="center">
  <img src="nlbn.svg" alt="nlbn logo" width="300"/>

  <h1>nlbn</h1>

  <p><strong>Fast EasyEDA/LCSC to KiCad converter written in Rust</strong></p>

  <p>Convert EasyEDA and LCSC components to KiCad library formats with blazing fast parallel downloads.</p>

  <p>
    <a href="https://crates.io/crates/nlbn"><img src="https://img.shields.io/crates/v/nlbn.svg" alt="Crates.io"></a>
    <a href="https://creativecommons.org/licenses/by-nc/4.0/"><img src="https://img.shields.io/badge/License-CC%20BY--NC%204.0-lightgrey.svg" alt="License: CC BY-NC 4.0"></a>
    <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/rust-1.70%2B-orange.svg" alt="Rust"></a>
  </p>

</div>

---

## Features

- ✅ Convert symbols, footprints, and 3D models 
- ✅ Batch processing with parallel downloads 
- ✅ Standalone binary - no dependencies required
- ✅ Low memory usage 

## Installation

**Note**: Currently tested on Windows platform only. Linux and macOS builds are provided but not yet tested.

### Option 1: Download Pre-built Binary

Download from [GitHub Releases](https://github.com/linkyourbin/nlbn/releases):
- Windows: `nlbn-windows-x86_64.exe.zip` ✓ Tested
- Linux: `nlbn-linux-x86_64.tar.gz` (untested)
- macOS: `nlbn-macos-x86_64.tar.gz` or `nlbn-macos-aarch64.tar.gz` (untested)

### Option 2: Install from crates.io

```bash
cargo install nlbn
```

### Option 3: Build from Source

```bash
git clone https://github.com/linkyourbin/nlbn.git
cd nlbn
cargo build --release
```

## Quick Start

### Single Component

```bash
# Convert everything (symbol + footprint + 3D model)
nlbn --full --lcsc-id C529356

# Convert only symbol
nlbn --symbol --lcsc-id C529356
```

### Batch Processing

```bash
# Create a file with LCSC IDs (one per line)
echo "C2040" > components.txt
echo "C529356" >> components.txt

# Batch convert with 8 parallel threads (fast!)
nlbn --full --batch components.txt --parallel 8

# Continue on errors
nlbn --full --batch components.txt --parallel 8 --continue-on-error
```

## Usage

```
nlbn [OPTIONS]

Options:
  --lcsc-id <ID>          LCSC component ID (e.g., C2040)
  --batch <FILE>          Batch mode: read IDs from file
  --symbol                Convert symbol only
  --footprint             Convert footprint only
  --3d                    Convert 3D model only
  --full                  Convert all (symbol + footprint + 3D)
  -o, --output <PATH>     Output directory [default: .]
  --parallel <N>          Parallel threads for batch mode [default: 4]
  --continue-on-error     Skip failed components in batch mode
  --overwrite             Overwrite existing components
  --v5                    Use KiCad v5 legacy format
  --debug                 Enable debug logging
  -h, --help              Print help
```

## Output Structure

```
output/
├── nlbn.kicad_sym              # Symbol library
├── nlbn.pretty/                # Footprint library
│   └── Component_Name.kicad_mod
└── nlbn.3dshapes/              # 3D model library
    └── Component_Name.step
```

## Examples

```bash
# High-performance batch conversion
nlbn --full --batch components.txt --parallel 16 -o ./library

# Resume interrupted batch (skip existing)
nlbn --full --batch components.txt --continue-on-error

# KiCad v5 format
nlbn --full --lcsc-id C529356 --v5
```

## License

This work is licensed under [CC BY-NC 4.0](https://creativecommons.org/licenses/by-nc/4.0/). You are free to share and adapt this work for non-commercial purposes with attribution.
