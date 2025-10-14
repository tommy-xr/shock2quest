# Architecture Overview

## System Shock 2 VR Port Architecture

This project ports System Shock 2 to VR, particularly Oculus Quest. The architecture is modular:

### Core Modules

- **Dark Engine Module** (`dark/`)
  - Handles original game file formats
  - Parsers for .bin, .mis, .cal, .gam files
  - Bridge between original assets and modern rendering

- **Rendering Engine** (`engine/`)
  - OpenGL-based rendering system
  - VR-optimized graphics pipeline
  - Cross-platform rendering abstractions

- **Game Logic** (`shock2vr/`)
  - Core gameplay systems
  - Object scripting system
  - Mission management
  - Save/load functionality
  - Creature AI and definitions

### Runtime Targets

- **Desktop Runtime** - Development and testing
- **Oculus Runtime** - Production VR target
- **Tool Runtime** - Asset viewing and debugging

## Key Design Principles

- Preserve original game mechanics while adapting for VR
- Maintain compatibility with original System Shock 2 assets
- Modular architecture for cross-platform support
- Performance optimization for mobile VR hardware