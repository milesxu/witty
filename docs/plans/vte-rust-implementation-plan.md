# VTE + Rust 完整实现计划

**日期**: 2026-03-22
**方案**: VTE + 自己实现所有模块（Rust）
**参考**: ghostty 源码
**目标**: 与 PySide6/QML 整合的完整终端

---

## 📋 项目概述

### 架构设计

```
┌─────────────────────────────────────────┐
│ Python (PySide6/QML)                    │
│  └─ QOpenGLWidget                       │
│      └─ Rust Terminal Library (PyO3)    │
└─────────────────────────────────────────┘
                    ↓
┌─────────────────────────────────────────┐
│ witty_terminal (Rust crate)             │
│                                         │
│ ┌─────────────────────────────────────┐ │
│ │ Terminal                            │ │
│ │  ├─ Grid (屏幕缓冲)                 │ │
│ │  ├─ Cursor (光标管理)               │ │
│ │  └─ Selection (选择)                │ │
│ └─────────────────────────────────────┘ │
│                                         │
│ ┌─────────────────────────────────────┐ │
│ │ Pty                                 │ │
│ │  └─ Unix Pty / Windows ConPTY      │ │
│ └─────────────────────────────────────┘ │
│                                         │
│ ┌─────────────────────────────────────┐ │
│ │ Renderer                            │ │
│ │  ├─ OpenGL 渲染器                   │ │
│ │  ├─ Font (rusttype/font-kit)       │ │
│ │  └─ Shaping (harfbuzz)             │ │
│ └─────────────────────────────────────┘ │
│                                         │
│ ┌─────────────────────────────────────┐ │
│ │ Input                               │ │
│ │  └─ 键盘/鼠标事件转换               │ │
│ └─────────────────────────────────────┘ │
└─────────────────────────────────────────┘
                    ↓
┌─────────────────────────────────────────┐
│ vte crate (VT 解析器)                   │
└─────────────────────────────────────────┘
```

---

## 🎯 需要实现的模块

基于 ghostty 分析，需要实现以下模块：

### 1. Terminal Core (~6k 行)
参考：`/home/xuming/src/ghostty/src/terminal/`

**组件**：
- `Term` - 终端状态机
- `Grid` - 字符网格（primary + alternate buffer）
- `Cursor` - 光标状态
- `Selection` - 文本选择
- `Mode` - 终端模式
- `Color` - 颜色管理

**工作量**：2 周

---

### 2. PTY Management (~2k 行)
参考：`/home/xuming/src/ghostty/src/pty.zig`

**组件**：
- `Pty` - PTY 抽象
- `PtyProcess` - 子进程管理
- Unix PTY 实现
- Windows ConPTY 实现（可选）

**工作量**：1 周

---

### 3. Renderer (~5k 行)
参考：`/home/xuming/src/ghostty/src/renderer/`

**组件**：
- `Renderer` - OpenGL 渲染器
- `QuadRenderer` - 批量四边形渲染
- `ShaderProgram` - 着色器管理
- `GlyphCache` - 字形缓存

**工作量**：2-3 周

---

### 4. Font System (~3k 行)
参考：`/home/xuming/src/ghostty/src/font/`

**组件**：
- `FontLoader` - 字体加载
- `FontAtlas` - 字形图集
- `TextShaper` - 文字 shaping (harfbuzz)
- `GlyphRasterizer` - 字形光栅化

**工作量**：1-2 周

---

### 5. Input Handling (~2k 行)
参考：`/home/xuming/src/ghostty/src/input/`

**组件**：
- `Keyboard` - 键盘事件转换
- `Mouse` - 鼠标事件转换
- `KeyEncoding` - 按键编码

**工作量**：1 周

---

### 6. Python Bindings (~2k 行)
参考：`/home/xuming/src/ghostty/macos/Sources/Ghostty/`

**组件**：
- PyO3 绑定
- Python API 设计
- 内存管理

**工作量**：1 周

---

### 总工作量：8-10 周
### 总代码量：~18k-20k 行 Rust

---

## 📅 详细实施计划

### Phase 1: 基础设施 (Week 1-2)

#### 1.1 项目结构创建

```
witty/
├── src/
│   └── witty_terminal/        # Rust crate
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs
│           ├── terminal/
│           │   ├── mod.rs
│           │   ├── term.rs
│           │   ├── grid.rs
│           │   ├── cursor.rs
│           │   └── selection.rs
│           ├── pty/
│           │   ├── mod.rs
│           │   └── unix.rs
│           ├── renderer/
│           │   ├── mod.rs
│           │   ├── renderer.rs
│           │   └── shader.rs
│           ├── font/
│           │   ├── mod.rs
│           │   └── atlas.rs
│           ├── input/
│           │   ├── mod.rs
│           │   └── keyboard.rs
│           └── python/
│               └── mod.rs
└── pyproject.toml
```

#### 1.2 Cargo.toml 配置

```toml
[package]
name = "witty-terminal"
version = "0.1.0"
edition = "2021"

[lib]
name = "witty_terminal"
crate-type = ["cdylib"]

[dependencies]
vte = "0.13"
# PTY
portable-pty = "0.8"
# Rendering
gl = "0.14"
# Font
rusttype = "0.9"
harfbuzz-rs = "2.0"
# Utilities
unicode-width = "0.1"
bitflags = "2.4"
log = "0.4"
# Python bindings
pyo3 = { version = "0.20", features = ["extension-module"] }

[build-dependencies]
pyo3-build-config = "0.20"
```

#### 1.3 交付物
- ✅ Rust 项目结构
- ✅ 依赖配置
- ✅ 编译脚本

---

### Phase 2: Terminal Core (Week 3-4)

#### 2.1 Grid 实现

**参考 ghostty**：`/home/xuming/src/ghostty/src/terminal/page.zig`

```rust
// src/terminal/grid.rs

/// 单个单元格
pub struct Cell {
    pub char: char,
    pub fg: Color,
    pub bg: Color,
    pub flags: CellFlags,
}

/// 字符网格
pub struct Grid {
    cols: usize,
    rows: usize,
    cells: Vec<Cell>,
    scrollback: VecDeque<Row>,
}

impl Grid {
    pub fn new(cols: usize, rows: usize) -> Self;

    pub fn resize(&mut self, cols: usize, rows: usize);

    pub fn cell(&self, col: usize, row: usize) -> &Cell;

    pub fn cell_mut(&mut self, col: usize, row: usize) -> &mut Cell;

    pub fn scroll_up(&mut self, count: usize);

    pub fn scroll_down(&mut self, count: usize);
}
```

**工作量**：3-4 天

---

#### 2.2 Cursor 实现

**参考 ghostty**：`/home/xuming/src/ghostty/src/terminal/cursor.zig`

```rust
// src/terminal/cursor.rs

pub struct Cursor {
    pub col: usize,
    pub row: usize,
    pub style: CursorStyle,
    pub visible: bool,
    pub blink: bool,
}

pub enum CursorStyle {
    Block,
    Underline,
    Bar,
}

impl Cursor {
    pub fn move_to(&mut self, col: usize, row: usize);

    pub fn move_up(&mut self, count: usize);

    pub fn move_down(&mut self, count: usize);
}
```

**工作量**：1-2 天

---

#### 2.3 Term 实现

**参考 ghostty**：`/home/xuming/src/ghostty/src/terminal/Terminal.zig`

```rust
// src/terminal/term.rs

pub struct Term {
    grid: Grid,
    alternate_grid: Grid,
    cursor: Cursor,
    saved_cursor: Cursor,
    mode: Mode,
    colors: ColorPalette,
    using_alternate_grid: bool,
}

impl Term {
    pub fn new(size: Size) -> Self;

    pub fn resize(&mut self, size: Size);

    pub fn input(&mut self, bytes: &[u8]);

    pub fn grid(&self) -> &Grid;

    pub fn grid_mut(&mut self) -> &mut Grid;
}
```

**关键实现**：实现 `vte::Perform` trait

```rust
impl vte::Perform for Term {
    fn print(&mut self, c: char) {
        // 写入字符到 grid
        let cell = self.grid.cell_mut(self.cursor.col, self.cursor.row);
        cell.char = c;
        self.cursor.col += 1;
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            0x08 => self.backspace(),  // BS
            0x09 => self.tab(),        // HT
            0x0A => self.linefeed(),   // LF
            0x0D => self.carriage_return(), // CR
            _ => {}
        }
    }

    fn csi_dispatch(
        &mut self,
        params: &vte::Params,
        intermediates: &[u8],
        ignore: bool,
        action: char,
    ) {
        // 处理 CSI 序列
        match action {
            'A' => self.cursor_up(params),
            'B' => self.cursor_down(params),
            'C' => self.cursor_forward(params),
            'D' => self.cursor_back(params),
            'H' => self.cursor_position(params),
            'J' => self.erase_display(params),
            'K' => self.erase_line(params),
            'm' => self.set_graphics_mode(params),
            _ => {}
        }
    }

    // ... 其他方法
}
```

**工作量**：1 周

---

#### 2.4 Selection 实现

```rust
// src/terminal/selection.rs

pub struct Selection {
    pub start: Option<Position>,
    pub end: Option<Position>,
    pub kind: SelectionKind,
}

pub enum SelectionKind {
    Normal,
    Line,
    Block,
}

impl Selection {
    pub fn start(&mut self, pos: Position);

    pub fn update(&mut self, pos: Position);

    pub fn is_selected(&self, pos: Position) -> bool;

    pub fn text(&self, grid: &Grid) -> String;
}
```

**工作量**：2-3 天

---

### Phase 3: PTY Management (Week 5)

#### 3.1 PTY 抽象

**参考 ghostty**：`/home/xuming/src/ghostty/src/pty.zig`

```rust
// src/pty/mod.rs

pub struct Pty {
    master: File,
    process: Child,
    size: Size,
}

impl Pty {
    pub fn new(
        config: PtyConfig,
        size: Size,
        env: HashMap<String, String>,
    ) -> Result<Self>;

    pub fn resize(&mut self, size: Size);

    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize>;

    pub fn write(&mut self, data: &[u8]) -> Result<()>;

    pub fn is_alive(&self) -> bool;
}

pub struct PtyConfig {
    pub shell: Option<String>,
    pub working_dir: Option<PathBuf>,
    pub env: HashMap<String, String>,
}
```

**Unix PTY 实现**：

```rust
// src/pty/unix.rs

use std::os::unix::io::AsRawFd;

impl Pty {
    pub fn new(config: PtyConfig, size: Size) -> Result<Self> {
        // 创建 PTY
        let (master_fd, slave_fd) = {
            let mut master: libc::c_int = 0;
            let mut slave: libc::c_int = 0;
            unsafe {
                libc::openpty(
                    &mut master,
                    &mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    &Winsize::from(size),
                );
            }
            (master, slave)
        };

        // Fork 子进程
        let pid = unsafe { libc::fork() };

        if pid == 0 {
            // 子进程
            libc::setsid();
            libc::ioctl(slave_fd, libc::TIOCSCTTY);
            libc::dup2(slave_fd, 0);
            libc::dup2(slave_fd, 1);
            libc::dup2(slave_fd, 2);

            // 执行 shell
            let shell = config.shell.unwrap_or_else(|| "bash".to_string());
            libc::execvp(/* ... */);
        } else {
            // 父进程
            Ok(Pty {
                master: unsafe { File::from_raw_fd(master_fd) },
                process: /* ... */,
                size,
            })
        }
    }
}
```

**工作量**：1 周

---

### Phase 4: Renderer (Week 6-8)

#### 4.1 OpenGL 渲染器基础

**参考 ghostty**：`/home/xuming/src/ghostty/src/renderer/OpenGL.zig`

```rust
// src/renderer/renderer.rs

pub struct Renderer {
    program: ShaderProgram,
    vao: gl::types::GLuint,
    vbo: gl::types::GLuint,
    glyph_cache: GlyphCache,
}

impl Renderer {
    pub fn new() -> Result<Self> {
        // 编译着色器
        let program = ShaderProgram::new(VERTEX_SHADER, FRAGMENT_SHADER)?;

        // 创建 VAO/VBO
        let (vao, vbo) = unsafe {
            let mut vao = 0;
            let mut vbo = 0;
            gl::GenVertexArrays(1, &mut vao);
            gl::GenBuffers(1, &mut vbo);
            (vao, vbo)
        };

        Ok(Self {
            program,
            vao,
            vbo,
            glyph_cache: GlyphCache::new()?,
        })
    }

    pub fn render(&mut self, term: &Term, size: Size) {
        unsafe {
            gl::Viewport(0, 0, size.width as i32, size.height as i32);
            gl::ClearColor(0.0, 0.0, 0.0, 1.0);
            gl::Clear(gl::COLOR_BUFFER_BIT);

            self.program.use_program();

            // 渲染所有单元格
            let vertices = self.build_vertices(term);
            self.render_vertices(&vertices);
        }
    }

    fn build_vertices(&mut self, term: &Term) -> Vec<Vertex> {
        let mut vertices = Vec::new();

        for row in 0..term.grid().rows {
            for col in 0..term.grid().cols {
                let cell = term.grid().cell(col, row);

                // 获取字形
                let glyph = self.glyph_cache.get(cell.char);

                // 添加顶点
                vertices.extend(glyph.vertices(col, row));
            }
        }

        vertices
    }
}
```

**着色器**：

```glsl
// vertex_shader.glsl
#version 330 core
layout (location = 0) in vec2 position;
layout (location = 1) in vec2 texcoord;
layout (location = 2) in vec4 color;

out vec2 TexCoord;
out vec4 Color;

uniform mat4 projection;

void main() {
    gl_Position = projection * vec4(position, 0.0, 1.0);
    TexCoord = texcoord;
    Color = color;
}
```

```glsl
// fragment_shader.glsl
#version 330 core
in vec2 TexCoord;
in vec4 Color;

out vec4 FragColor;

uniform sampler2D glyphTexture;

void main() {
    float alpha = texture(glyphTexture, TexCoord).r;
    FragColor = vec4(Color.rgb, Color.a * alpha);
}
```

**工作量**：1 周

---

#### 4.2 字形缓存

```rust
// src/renderer/glyph_cache.rs

pub struct GlyphCache {
    texture: gl::types::GLuint,
    atlas: FontAtlas,
}

impl GlyphCache {
    pub fn new() -> Result<Self>;

    pub fn get(&mut self, char: char) -> &Glyph {
        if !self.atlas.contains(char) {
            self.cache_glyph(char);
        }
        self.atlas.get(char)
    }

    fn cache_glyph(&mut self, char: char) {
        // 从字体渲染字形
        let glyph = self.atlas.font.render(char);

        // 上传到纹理图集
        self.upload_to_atlas(glyph);
    }
}
```

**工作量**：3-4 天

---

### Phase 5: Font System (Week 8-9)

#### 5.1 字体加载和渲染

**使用 rusttype**：

```rust
// src/font/mod.rs

use rusttype::{Font, Scale, PositionedGlyph};

pub struct FontLoader {
    font: Font<'static>,
    size: f32,
}

impl FontLoader {
    pub fn new(path: &Path, size: f32) -> Result<Self> {
        let font_data = std::fs::read(path)?;
        let font = Font::try_from_vec(font_data)?;

        Ok(Self { font, size })
    }

    pub fn render(&self, char: char) -> Glyph {
        let scale = Scale::uniform(self.size);
        let glyph = self.font.glyph(char).scaled(scale).positioned(point(0.0, 0.0));

        // 光栅化
        let pixel_bb = glyph.pixel_bounding_box().unwrap();
        let mut pixels = vec![0u8; pixel_bb.width() as usize * pixel_bb.height() as usize];

        glyph.draw(|x, y, v| {
            pixels[(y * pixel_bb.width() as u32 + x) as usize] = (v * 255.0) as u8;
        });

        Glyph {
            width: pixel_bb.width() as u32,
            height: pixel_bb.height() as u32,
            pixels,
        }
    }
}
```

// 使用harfbuzz, 不要使用上述rusttype
**或使用 font-kit + harfbuzz**（更专业）：

```rust
// src/font/shaper.rs

use harfbuzz_rs::*;

pub struct TextShaper {
    hb_font: Owned<Font>,
}

impl TextShaper {
    pub fn shape(&self, text: &str) -> Vec<PositionedGlyph> {
        let buffer = UnicodeBuffer::new()
            .add_str(text)
            .guess_segment_properties();

        let output = self.hb_font.shape(buffer, &[]);

        output.get_glyph_positions()
            .iter()
            .map(|pos| PositionedGlyph {
                glyph_id: pos.codepoint,
                x_offset: pos.x_offset,
                y_offset: pos.y_offset,
                x_advance: pos.x_advance,
            })
            .collect()
    }
}
```

**工作量**：1-2 周

---

### Phase 6: Input Handling (Week 9)

#### 6.1 键盘事件

**参考 ghostty**：`/home/xuming/src/ghostty/src/input/Key.zig`

```rust
// src/input/keyboard.rs

pub struct KeyEvent {
    pub key: Key,
    pub modifiers: Modifiers,
    pub text: Option<String>,
}

pub enum Key {
    Char(char),
    Escape,
    Enter,
    Backspace,
    Tab,
    // ... 其他键
}

pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub super_: bool,
}

impl KeyEvent {
    pub fn encode(&self) -> Vec<u8> {
        // 将按键编码为终端序列
        match self.key {
            Key::Char(c) => {
                if self.modifiers.ctrl {
                    // Ctrl 组合键
                    encode_ctrl_key(c)
                } else {
                    vec![c as u8]
                }
            }
            Key::Escape => vec![0x1B],
            Key::Enter => vec![0x0D],
            Key::Backspace => vec![0x7F],
            // ... 其他键
        }
    }
}

fn encode_ctrl_key(c: char) -> Vec<u8> {
    // Ctrl 键编码规则
    let code = (c as u8) & 0x1F;
    vec![code]
}
```

**工作量**：3-4 天

---

### Phase 7: Python Bindings (Week 10)

#### 7.1 PyO3 绑定

```rust
// src/python/mod.rs

use pyo3::prelude::*;
use pyo3::types::*;

#[pyclass]
pub struct Terminal {
    term: Term,
    pty: Pty,
    renderer: Renderer,
}

#[pymethods]
impl Terminal {
    #[new]
    pub fn new(cols: usize, rows: usize) -> PyResult<Self> {
        let term = Term::new(Size::new(cols, rows));
        let pty = Pty::new(PtyConfig::default(), Size::new(cols, rows))?;
        let renderer = Renderer::new()?;

        Ok(Self { term, pty, renderer })
    }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        self.term.resize(Size::new(cols, rows));
        self.pty.resize(Size::new(cols, rows));
    }

    pub fn process(&mut self) -> PyResult<()> {
        // 从 PTY 读取数据
        let mut buf = [0u8; 4096];
        let n = self.pty.read(&mut buf)?;

        // 处理输入
        self.term.input(&buf[..n]);

        Ok(())
    }

    pub fn render(&mut self) {
        self.renderer.render(&self.term, self.term.size());
    }

    pub fn send_key(&mut self, key: &str, ctrl: bool, alt: bool, shift: bool) {
        let event = KeyEvent::from_string(key, ctrl, alt, shift);
        let bytes = event.encode();
        self.pty.write(&bytes).ok();
    }

    pub fn send_text(&mut self, text: &str) {
        self.pty.write(text.as_bytes()).ok();
    }
}

#[pymodule]
fn witty_terminal(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_class::<Terminal>()?;
    Ok(())
}
```

**工作量**：1 周

---

### Phase 8: QML Integration (Week 10-11)

#### 8.1 QOpenGLWidget 实现

```python
# src/witty/gui/terminal_widget.py

from PySide6.QtCore import Qt, QTimer, Signal
from PySide6.QtWidgets import QWidget
from PySide6.QtGui import QSurfaceFormat
from PySide6.QtOpenGL import QOpenGLWidget

import witty_terminal

class TerminalWidget(QOpenGLWidget):
    """QML-compatible terminal widget using Rust backend."""

    title_changed = Signal(str)

    def __init__(self, parent=None):
        super().__init__(parent)

        # 设置 OpenGL 格式
        fmt = QSurfaceFormat()
        fmt.setVersion(4, 3)
        fmt.setProfile(QSurfaceFormat.CoreProfile)
        self.setFormat(fmt)

        # 初始化 Rust 终端
        self.terminal = None
        self.cols = 80
        self.rows = 24

        # 定时器处理 PTY 输出
        self.timer = QTimer(self)
        self.timer.timeout.connect(self.process)
        self.timer.start(16)  # 60 FPS

    def initializeGL(self):
        """初始化 OpenGL 上下文。"""
        # 创建终端实例
        self.terminal = witty_terminal.Terminal(self.cols, self.rows)

    def paintGL(self):
        """渲染终端。"""
        if self.terminal:
            self.terminal.render()

    def resizeGL(self, width, height):
        """处理大小变化。"""
        if self.terminal:
            # 计算新的行列数
            cell_width = 10
            cell_height = 20
            cols = width // cell_width
            rows = height // cell_height

            self.terminal.resize(cols, rows)
            self.cols = cols
            self.rows = rows

    def process(self):
        """处理 PTY 输出。"""
        if self.terminal:
            self.terminal.process()
            self.update()  # 触发重绘

    def keyPressEvent(self, event):
        """处理键盘输入。"""
        if self.terminal:
            modifiers = event.modifiers()
            ctrl = bool(modifiers & Qt.ControlModifier)
            alt = bool(modifiers & Qt.AltModifier)
            shift = bool(modifiers & Qt.ShiftModifier)

            key_text = event.text()
            if key_text:
                self.terminal.send_text(key_text)
            else:
                # 特殊键
                key_name = self.key_to_name(event.key())
                self.terminal.send_key(key_name, ctrl, alt, shift)

    def key_to_name(self, key):
        """将 Qt 键码转换为键名。"""
        mapping = {
            Qt.Key_Escape: "Escape",
            Qt.Key_Return: "Enter",
            Qt.Key_Backspace: "Backspace",
            Qt.Key_Tab: "Tab",
            Qt.Key_Up: "Up",
            Qt.Key_Down: "Down",
            Qt.Key_Left: "Left",
            Qt.Key_Right: "Right",
            # ... 其他键
        }
        return mapping.get(key, "")
```

#### 8.2 QML 组件

```qml
// src/witty/gui/TerminalView.qml

import QtQuick
import QtQuick.Controls
import witty.terminal 1.0

ApplicationWindow {
    visible: true
    width: 800
    height: 600
    title: "Witty Terminal"

    TerminalWidget {
        id: terminal
        anchors.fill: parent

        onTitleChanged: {
            window.title = title
        }
    }

    Shortcut {
        sequence: "Ctrl+Shift+C"
        onActivated: terminal.copy()
    }

    Shortcut {
        sequence: "Ctrl+Shift+V"
        onActivated: terminal.paste()
    }
}
```

**工作量**：1 周

---

## 📊 工作量汇总

| 阶段     | 模块            | 工作量       | 代码量估计  |
| -------- | --------------- | ------------ | ----------- |
| Phase 1  | 基础设施        | 1 周         | ~500 行     |
| Phase 2  | Terminal Core   | 2 周         | ~6k 行      |
| Phase 3  | PTY             | 1 周         | ~2k 行      |
| Phase 4  | Renderer        | 2-3 周       | ~5k 行      |
| Phase 5  | Font            | 1-2 周       | ~3k 行      |
| Phase 6  | Input           | 1 周         | ~2k 行      |
| Phase 7  | Python Bindings | 1 周         | ~2k 行      |
| Phase 8  | QML Integration | 1 周         | ~1k 行      |
| **总计** |                 | **10-12 周** | **~21k 行** |

---

## ✅ 里程碑和验证点

### Milestone 1: Terminal Core 可用 (Week 4)
**验证**：
```rust
let mut term = Term::new(Size::new(80, 24));
term.input(b"Hello, World!\n");
assert_eq!(term.grid().cell(0, 0).char, 'H');
```

### Milestone 2: PTY 集成 (Week 5)
**验证**：
```rust
let mut pty = Pty::new(PtyConfig::default(), Size::new(80, 24))?;
pty.write(b"ls\n")?;
let mut buf = [0u8; 1024];
let n = pty.read(&mut buf)?;
assert!(n > 0);
```

### Milestone 3: 渲染器基础 (Week 7)
**验证**：
- 显示文本
- 正确处理颜色

### Milestone 4: 字体渲染 (Week 9)
**验证**：
- 字符显示清晰
- 支持 Unicode

### Milestone 5: Python 绑定 (Week 10)
**验证**：
```python
import witty_terminal
term = witty_terminal.Terminal(80, 24)
term.send_text("ls\n")
```

### Milestone 6: QML 集成 (Week 11)
**验证**：
- 在 QML 窗口中显示终端
- 可以输入命令
- 可以看到输出

---

## 🔧 开发工具和流程

### 开发环境

```bash
# 安装 Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 安装 Python 依赖
uv add maturin

# 初始化 Rust 项目
cd src/witty_terminal
cargo init --lib
```

### 构建流程

```bash
# 编译 Rust 库
cd src/witty_terminal
maturin develop

# 运行 Python 测试
python -c "import witty_terminal; t = witty_terminal.Terminal(80, 24)"
```

### 测试策略

1. **单元测试**（Rust）
```rust
#[test]
fn test_grid_resize() {
    let mut grid = Grid::new(80, 24);
    grid.resize(100, 30);
    assert_eq!(grid.cols, 100);
    assert_eq!(grid.rows, 30);
}
```

2. **集成测试**（Python）
```python
def test_terminal_input():
    term = Terminal(80, 24)
    term.send_text("echo test\n")
    time.sleep(0.1)
    term.process()
    # 验证输出
```

3. **端到端测试**（QML）
- 启动应用
- 输入命令
- 验证显示

---

## 📚 参考文档

### Ghostty 源码
- Terminal: `/home/xuming/src/ghostty/src/terminal/`
- PTY: `/home/xuming/src/ghostty/src/pty.zig`
- Renderer: `/home/xuming/src/ghostty/src/renderer/`
- Font: `/home/xuming/src/ghostty/src/font/`

### Rust 文档
- vte: https://docs.rs/vte
- PyO3: https://pyo3.rs
- rusttype: https://docs.rs/rusttype

### OpenGL
- Learn OpenGL: https://learnopengl.com
- Qt OpenGL: https://doc.qt.io/qt-6/qopenglwidget.html

---

## 🎯 最终交付物

1. ✅ `witty-terminal` Rust crate
2. ✅ Python 模块 `witty_terminal`
3. ✅ QML Terminal 组件
4. ✅ 完整文档
5. ✅ 测试套件
6. ✅ 示例应用

---

**总结**：这是一个完整的、自包含的实现方案，工作量约 10-12 周。最终将拥有完全自主可控的终端实现，可以深度定制和优化。