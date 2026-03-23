# libghostty 技术分析文档

**文档版本**: 1.0
**日期**: 2026-03-22
**作者**: Claude
**目的**: 为 Qt6/QML 集成 libghostty 提供技术基础

## 1. 架构概览

### 1.1 libghostty 设计理念

Ghostty 采用模块化架构，核心特点：
- **跨平台核心**: Zig 编写的共享核心处理终端仿真、渲染和 I/O
- **平台原生 UI**: macOS (Swift/SwiftUI), Linux (GTK4)
- **C API 嵌入支持**: libghostty 暴露 C API 用于集成

### 1.2 主要组件

#### libghostty (完整库)
**位置**: `/home/xuming/src/ghostty/include/ghostty.h`

主要 C API 类型：
```c
typedef void* ghostty_app_t;       // 应用实例
typedef void* ghostty_config_t;    // 配置对象
typedef void* ghostty_surface_t;   // 终端表面（widget）
typedef void* ghostty_inspector_t; // 检查器
```

关键 API 函数：

**初始化**:
- `ghostty_init()` - 初始化库
- `ghostty_app_new()` - 创建应用实例
- `ghostty_app_free()` - 释放应用
- `ghostty_app_tick()` - 事件循环 tick

**Surface 管理**:
- `ghostty_surface_new()` - 创建终端表面
- `ghostty_surface_free()` - 释放表面
- `ghostty_surface_draw()` - 绘制帧
- `ghostty_surface_set_size()` - 设置尺寸

**输入处理**:
- `ghostty_surface_key()` - 键盘输入
- `ghostty_surface_text()` - 文本输入
- `ghostty_surface_mouse_button()` - 鼠标按钮
- `ghostty_surface_mouse_pos()` - 鼠标位置
- `ghostty_surface_mouse_scroll()` - 滚动

**配置**:
- `ghostty_config_new()` - 创建配置
- `ghostty_config_load_file()` - 加载配置文件
- `ghostty_config_get()` - 获取配置项

#### libghostty-vt (终端仿真库)
**位置**: `/home/xuming/src/ghostty/include/ghostty/vt.h`

专注终端仿真，不包含渲染：
- VT Parser (`Parser`)
- Terminal state (`Terminal`, `Screen`, `PageList`)
- Key encoding (`ghostty/vt/key.h`)
- OSC/SGR parsing
- Paste validation

## 2. 核心源码结构

**位置**: `/home/xuming/src/ghostty/src/`

| 目录/文件 | 功能 |
|---------|------|
| `Surface.zig` | 终端 widget 抽象 - 管理 PTY, 处理 I/O, 渲染 |
| `App.zig` | 应用实例，管理 surfaces, 字体缓存 |
| `terminal/` | VT 解析, 屏幕状态, ANSI/CSI/OSC 处理 |
| `renderer/` | 多渲染器架构 (OpenGL/Metal) |
| `font/` | 字体发现、加载、shaping (HarfBuzz, FreeType) |
| `config/` | 配置解析和验证 |
| `apprt/` | 应用运行时抽象 (GTK, embedded) |
| `termio/` | PTY I/O 处理, shell 集成 |
| `input/` | 键盘/鼠标输入处理 |
| `pty.zig` | PTY 创建和管理 |

## 3. 渲染管道

### 3.1 架构
渲染器使用泛型架构，支持可插拔后端：

**位置**: `/home/xuming/src/ghostty/src/renderer/generic.zig`

```zig
pub fn Renderer(comptime GraphicsAPI: type) type {
    // 泛型渲染器，支持任何图形 API
}
```

**后端**:
- **OpenGL** (`renderer/OpenGL.zig`): Linux, 需要 OpenGL 4.3+
- **Metal** (`renderer/Metal.zig`): macOS

### 3.2 渲染组件层次

```
[ GraphicsAPI ] - 配置 surface, 提供 render Targets
    |
    V
[ Target ] - 抽象渲染目标 (surface 或 framebuffer)
    |
    V
[ Frame ] - 帧绘制上下文
    |
    V
[ RenderPass ] - 应用于相同目标的一个或多个 Step
    |
    V
[ Pipeline ] - 顶点/片段着色器
```

**关键渲染文件**:
- `/home/xuming/src/ghostty/src/renderer/generic.zig` - 主渲染器逻辑
- `/home/xuming/src/ghostty/src/renderer/OpenGL.zig` - OpenGL 后端
- `/home/xuming/src/ghostty/src/renderer/Metal.zig` - Metal 后端
- `/home/xuming/src/ghostty/src/renderer/shaders/` - GLSL 着色器

### 3.3 字体渲染
**位置**: `/home/xuming/src/ghostty/src/font/`

- 使用 **FreeType** 加载字体
- **HarfBuzz** 进行文本 shaping (连字)
- `font/SharedGrid.zig` - 字形缓存
- `font/shaper/` - 文本 shaping 实现

## 4. 终端仿真

### 4.1 VT Parser
**位置**: `/home/xuming/src/ghostty/src/terminal/Parser.zig`

基于 [vt100.net parser](https://vt100.net/emu/dec_ansi_parser) 的状态机：

```zig
pub const State = enum {
    ground, escape, escape_intermediate,
    csi_entry, csi_intermediate, csi_param, csi_ignore,
    dcs_entry, dcs_param, dcs_intermediate, dcs_passthrough, dcs_ignore,
    osc_string, sos_pm_apc_string,
};
```

### 4.2 屏幕管理
**位置**: `/home/xuming/src/ghostty/src/terminal/`

- `Terminal.zig` - 主终端状态 (光标, 模式, 颜色)
- `Screen.zig` - 屏幕缓冲 (主缓冲/备用缓冲)
- `PageList.zig` - 滚动缓冲
- `page.zig` - 单个页面的单元格

### 4.3 流处理
**位置**: `/home/xuming/src/ghostty/src/terminal/stream.zig`

解析器发出的动作包括：
- `print`, `bell`, `backspace`, `linefeed`, `carriage_return`
- `cursor_up/down/left/right`, `cursor_pos`
- `erase_display_*`, `erase_line_*`
- `set_mode`, `reset_mode`
- OSC/CSI/DCS 命令

## 5. PTY 处理

**位置**: `/home/xuming/src/ghostty/src/pty.zig`

平台特定 PTY：
- **POSIX** (`PosixPty`): 使用 `openpty()`, `fork()`, `setsid()`
- **Windows** (`WindowsPty`): 使用 ConPTY API

```zig
pub const Pty = switch (builtin.os.tag) {
    .windows => WindowsPty,
    .ios => NullPty,
    else => PosixPty,
};
```

**Termio** (`/home/xuming/src/ghostty/src/termio/`):
- `Termio.zig` - 主 I/O 处理器
- `Thread.zig` - 多线程 I/O
- `stream_handler.zig` - 处理终端输出

## 6. 嵌入示例

### 6.1 macOS 集成 (实际案例)
**位置**: `/home/xuming/src/ghostty/macos/Sources/Ghostty/`

macOS 应用展示了**嵌入模式**：

1. **Ghostty.App.swift** - 管理 `ghostty_app_t`
```swift
@Published var app: ghostty_app_t?
```

2. **Ghostty.Surface.swift** - 封装 `ghostty_surface_t`
```swift
final class Surface: Sendable {
    private let surface: ghostty_surface_t

    func sendText(_ text: String) {
        ghostty_surface_text(surface, ptr, UInt(len - 1))
    }
}
```

3. **SurfaceView.swift** - 渲染终端的 SwiftUI 视图
```swift
SurfaceRepresentable(view: surfaceView, size: geo.size)
```

## 7. 构建信息

**构建系统**: Zig build (`/home/xuming/src/ghostty/build.zig`)

```bash
# 构建 libghostty (共享库)
zig build -Dapp-runtime=none

# 仅构建 libghostty-vt
zig build lib-vt

# 构建 macOS xcframework
zig build -Dtarget=aarch64-macos -Dapp-runtime=none
```

**输出库**:
- `libghostty.so` / `libghostty.a` - 完整库
- `libghostty-vt.so` / `libghostty-vt.a` - 仅终端仿真

## 8. 依赖项

从 `/home/xuming/src/ghostty/pkg/` 查看依赖：

- **FreeType** - 字体渲染
- **HarfBuzz** - 文本 shaping
- **oniguruma** - 正则表达式搜索
- **zlib** - 压缩
- **libxml2** - XML 解析
- **libpng** - PNG 支持

## 9. 平台要求

- **Linux**: OpenGL 4.3+
- **macOS**: Metal
- **Zig 版本**: 0.15.2+ (查看 `build.zig.zon.minimum_zig_version`)

## 10. API 稳定性说明

嵌入 API 标记为"尚未稳定"，但已被 macOS 应用主动使用。

## 11. Qt6/QML 集成的关键集成点

### 方案 A: 完整 libghostty (推荐快速启动)

1. **初始化**:
```cpp
ghostty_config_t config = ghostty_config_new();
ghostty_config_load_file(config, "/path/to/config");
ghostty_app_t app = ghostty_app_new(&runtime_config, config);
```

2. **创建 surface**:
```cpp
ghostty_surface_config_s surface_config = ghostty_surface_config_new();
ghostty_surface_t surface = ghostty_surface_new(app, &surface_config);
```

3. **渲染循环**:
   - 每帧调用 `ghostty_surface_draw()`
   - 需要通过平台特定配置提供 Metal/OpenGL 上下文

4. **输入处理**:
   - 将 Qt 键盘/鼠标事件转换为 `ghostty_input_key_s` 等

5. **实现回调**:
   - `action_cb` - 处理动作 (新建窗口, 复制, 粘贴等)
   - `read_clipboard_cb` / `write_clipboard_cb` - 剪贴板集成
   - `wakeup_cb` - 唤醒 Qt 事件循环

### 方案 B: libghostty-vt + 自定义渲染

如果你想要完全控制渲染：

1. 使用 libghostty-vt 的 `Terminal`, `Stream`
2. 自己处理 VT 解析
3. 使用 ghostty 的字体系统实现自己的 OpenGL 渲染器

## 12. 关键源文件参考

| 文件 | 用途 |
|-----|------|
| `/home/xuming/src/ghostty/include/ghostty.h` | 主 C API 头文件 |
| `/home/xuming/src/ghostty/src/apprt/embedded.zig` | 嵌入运行时实现 |
| `/home/xuming/src/ghostty/src/Surface.zig` | Surface 实现 |
| `/home/xuming/src/ghostty/src/renderer/OpenGL.zig` | OpenGL 渲染器 |
| `/home/xuming/src/ghostty/macos/Sources/Ghostty/Ghostty.Surface.swift` | 实际嵌入示例 |
| `/home/xuming/src/ghostty/macos/Sources/Ghostty/SurfaceView/SurfaceView_AppKit.swift` | AppKit Metal 渲染 |

---

**下一步**: 参见 `docs/plans/qt6-integration-plan.md` 了解具体实施计划