# Witty Terminal 💡

**Witty** is a modern, AI-powered terminal emulator built with Python, Qt6/QtQuick, and Rust.

> "Terminal with a brain"

## Features

- 🚀 **High Performance** - Powered by libghostty for rendering
- 🤖 **AI Integration** - Intelligent command assistance and auto-completion
- 🎨 **Modern UI** - Built with Qt6 Quick/QML for a beautiful interface
- 🔧 **Hybrid Architecture** - Python for rapid development, Rust for performance-critical paths
- 🌍 **Cross-platform** - Linux, macOS, and Windows support

## Architecture

```
┌─────────────────────────────────────┐
│  Python (PySide6/QtQuick)           │
│  • UI Layer                         │
│  • Business Logic                   │
│  • Configuration                    │
└─────────────────────────────────────┘
            ↕ PyO3
┌─────────────────────────────────────┐
│  Rust Core                          │
│  • PTY Processing                   │
│  • Performance Optimization         │
│  • Data Bridge                      │
└─────────────────────────────────────┘
            ↕ FFI
┌─────────────────────────────────────┐
│  libghostty                         │
│  • Terminal Rendering               │
│  • Font Engine                      │
│  • VT Parser                        │
└─────────────────────────────────────┘
```

## Tech Stack

- **UI Framework**: Qt6 Quick (QML) + PySide6
- **Core Engine**: libghostty v1.3.1+
- **Performance Layer**: Rust (PyO3)
- **Terminal Backend**: libghostty-vt

## Quick Start

```bash
# Clone the repository
git clone https://github.com/yourusername/witty.git
cd witty

# Install dependencies
pip install -r requirements.txt

# Run
python -m witty
```

## Development Status

🚧 **Early Development** - This project is currently in the planning/prototyping phase.

## License

MIT License - see [LICENSE](LICENSE) for details.

## Inspiration

Witty is inspired by:
- [Ghostty](https://github.com/ghostty-org/ghostty) - High-performance terminal emulator
- [Alacritty](https://github.com/alacritty/alacritty) - GPU-accelerated terminal
- [Kitty](https://github.com/kovidgoyal/kitty) - GPU-based terminal

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

---

**Witty Terminal** - Speed meets intelligence 💡
