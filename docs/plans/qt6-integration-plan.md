# PySide6/QML 集成 libghostty 实施计划

**文档版本**: 2.0
**日期**: 2026-03-22
**状态**: 规划阶段（已更新为 Python/PySide6 方案）
**目标**: 使用 PySide6/QML 调用 libghostty，实现终端窗口显示和初步交互

## 📋 项目目标

### 阶段目标
1. **显示终端窗口** - 在 PySide6 QQuick 窗口中渲染终端内容
2. **初步交互** - 支持键盘输入和基本的终端操作
3. **文档化** - 每个需求都有可追溯的文档

### 最终目标
- 将 PySide6 对 ghostty 的调用整理成可复用的 skill
- 建立完善的文档体系，支持随时查阅

---

## 🎯 实施路线图

### Phase 1: 环境准备和基础集成 (Week 1-2)

#### 1.1 依赖安装和验证
- [x] PySide6 已安装 (6.7.3)
- [x] Python 3.14.3 已安装
- [x] cffi 已安装 (用于调用 C API)
- [ ] 验证 libghostty 1.3.1 安装
  - 检查库文件: `/usr/lib/libghostty.so`
  - 检查头文件: `/usr/include/ghostty.h`
- [ ] 安装 PyOpenGL（如需要）
  ```bash
  uv add PyOpenGL PyOpenGL-accelerate
  ```

#### 1.2 Python FFI 绑定层
- [ ] 创建 `src/witty/ghostty/` 目录
- [ ] 创建 `ghostty_bindings.py` 使用 cffi 加载 libghostty
  ```python
  from cffi import FFI

  ffi = FFI()
  # 定义 ghostty.h 的 C API
  ffi.cdef("""
      typedef void* ghostty_app_t;
      typedef void* ghostty_surface_t;
      // ... API 定义
  """)

  lib = ffi.dlopen("libghostty.so")
  ```

#### 1.3 封装 Python 类
- [ ] 实现 `GhosttyApp` Python 类
  - 封装 `ghostty_app_t`
  - 管理生命周期
  - 使用 ctypes/cffi 调用 C 函数
- [ ] 实现 `GhosttySurface` Python 类
  - 封装 `ghostty_surface_t`
  - 管理 surface 生命周期

**交付物**:
- Python 绑定层代码
- 可调用 libghostty API 的 Python 模块

---

### Phase 2: PySide6 OpenGL 渲染集成 (Week 3-4)

#### 2.1 QQuickItem 子类实现（Python）
- [ ] 创建 `GhosttyTerminal` QQuickItem
  - 继承 `PySide6.QtQuick.QQuickItem`
  - 重写 `updatePaintNode()` 方法
  - 使用 PySide6 的 Scene Graph API
- [ ] OpenGL 上下文管理
  - 使用 `QOpenGLContext` 和 `QOpenGLFunctions`
  - 配置 OpenGL 4.3+ 核心模式
  - 传递上下文给 libghostty

#### 2.2 渲染循环实现
- [ ] 实现 `ghostty_surface_draw()` 调用
  - 在 Qt 渲染线程中调用
  - 使用 PySide6 的渲染信号
- [ ] 实现 `ghostty_surface_set_size()` 响应
  - 监听窗口大小变化
  - 更新终端尺寸

**交付物**:
- 能显示空白终端窗口的 QML 应用
- Python 实现的 OpenGL 渲染集成

---

### Phase 3: PTY 和 Shell 启动 (Week 5)

#### 3.1 配置管理
- [ ] 实现 `ghostty_config_t` 配置加载（Python）
  - 创建默认配置
  - 设置 shell 路径
  - 配置环境变量
- [ ] 启动 PTY 进程
  - 通过 libghostty API 启动 shell

#### 3.2 终端初始化
- [ ] 设置终端尺寸 (默认 80x24)
- [ ] 验证 shell 提示符显示

**交付物**:
- 显示 shell 提示符的终端窗口

---

### Phase 4: 输入处理 (Week 6)

#### 4.1 键盘输入
- [ ] 使用 PySide6 的键盘事件
  - `keyPressEvent` / `keyReleaseEvent`
  - 转换为 `ghostty_input_key_s` 结构
- [ ] 调用 `ghostty_surface_key()` (通过 cffi)

#### 4.2 鼠标输入
- [ ] 使用 PySide6 的鼠标事件
  - `mousePressEvent` / `mouseReleaseEvent`
  - `mouseMoveEvent`
  - `wheelEvent`
- [ ] 转换并调用 ghostty 鼠标 API

**交付物**:
- 可交互的终端窗口
- 基本的命令执行能力

---

### Phase 5: 回调和事件循环 (Week 7)

#### 5.1 实现运行时回调（Python）
- [ ] 使用 Python 回调函数
  - `wakeup_cb` - 使用 QTimer
  - `action_cb` - 处理 ghostty 动作
  - `read_clipboard_cb` / `write_clipboard_cb` - 使用 QClipboard
  - `close_surface_cb` - 关闭 surface

#### 5.2 事件循环集成
- [ ] 集成 `ghostty_app_tick()` 到 Qt 事件循环
  - 使用 `QTimer` 定期调用

**交付物**:
- 完整的终端交互功能
- 剪贴板支持

---

### Phase 6: QML API 设计 (Week 8)

#### 6.1 QML 类型注册（Python）
- [ ] 使用 `qmlRegisterType` 注册 Python 类
  ```python
  from PySide6.QtQml import qmlRegisterType

  qmlRegisterType(GhosttyTerminal, "Witty.Ghostty", 1, 0, "Terminal")
  ```
- [ ] 定义 Q_PROPERTY
  - `fontSize`: int
  - `fontFamily`: string
  - `shell`: string

#### 6.2 QML 组件设计
- [ ] 创建 `TerminalView.qml`
  - 封装 GhosttyTerminal
  - 添加工具栏

**交付物**:
- 可在 QML 中使用的 Terminal 组件

---

### Phase 7: 测试和文档 (Week 9)

#### 7.1 测试
- [ ] Python 单元测试
  - GhosttyApp 测试
  - GhosttySurface 测试
  - 输入转换测试
- [ ] QML 集成测试

#### 7.2 文档完善
- [ ] API 文档（Python）
- [ ] 集成指南
- [ ] 架构文档

**交付物**:
- 完整的测试套件
- 完善的文档

---

## 🛠️ 技术决策

### 决策 1: 渲染方案
**选项**:
- A: 使用完整 libghostty + OpenGL 后端 (推荐)
- B: 使用 libghostty-vt + 自定义渲染

**选择**: 方案 A

**理由**:
- 开发速度快
- 利用 ghostty 成熟的渲染管道
- 减少维护负担

### 决策 2: 构建系统
**选项**:
- A: CMake
- B: Meson
- C: Zig build

**选择**: CMake

**理由**:
- Qt6 原生支持
- PySide6 集成良好
- 社区支持广泛

### 决策 3: Python 绑定
**选项**:
- A: 仅 C++ + QML
- B: C++ + PySide6 绑定

**选择**: 初期使用 A，后续可添加 B

**理由**:
- 降低初期复杂度
- 性能考虑
- QML 提供足够的灵活性

---

## 📂 目录结构规划

```
src/
├── witty/
│   ├── __init__.py              # Python 入口
│   ├── gui/                     # QML GUI
│   │   ├── __init__.py
│   │   └── main.qml
│   └── ghostty/                 # C++ 绑定层
│       ├── CMakeLists.txt
│       ├── ghostty_app.h        # App 封装
│       ├── ghostty_app.cpp
│       ├── ghostty_surface.h    # Surface 封装
│       ├── ghostty_surface.cpp
│       ├── ghostty_terminal.h   # QQuickItem
│       ├── ghostty_terminal.cpp
│       └── input_converter.h    # 输入转换
└── CMakeLists.txt               # 主构建配置

docs/
├── integration/                 # 集成文档
│   ├── setup.md
│   ├── building.md
│   └── troubleshooting.md
├── plans/                       # 计划文档
│   ├── qt6-integration-plan.md
│   └── phase-1-checklist.md
├── technical/                   # 技术文档
│   ├── libghostty-analysis.md
│   ├── rendering-pipeline.md
│   └── input-handling.md
└── api/                         # API 文档
    ├── cpp-api.md
    └── qml-api.md
```

---

## 🎓 技能文档化策略

每个技术要点整理成独立的 skill 文档：

1. **ghostty-opengl-integration** - OpenGL 集成模式
2. **ghostty-input-translation** - Qt 输入转 ghostty 输入
3. **ghostty-callback-handling** - 回调处理模式
4. **ghostty-configuration** - 配置管理
5. **ghostty-font-rendering** - 字体渲染集成

每个 skill 包含：
- 概述
- 代码示例
- 常见问题
- 调试技巧

---

## ✅ 验收标准

### Phase 1-2 验收
- [ ] 项目可编译
- [ ] 显示空白终端窗口
- [ ] OpenGL 上下文正确

### Phase 3-4 验收
- [ ] 显示 shell 提示符
- [ ] 可输入命令
- [ ] 命令输出正确显示

### Phase 5-6 验收
- [ ] 可复制粘贴文本
- [ ] 鼠标选择工作正常
- [ ] QML API 易用

### Phase 7 验收
- [ ] 测试覆盖率 > 70%
- [ ] 文档完整
- [ ] 示例应用功能完整

---

## 🚨 风险和缓解

### 风险 1: OpenGL 版本兼容性
**问题**: libghostty 需要 OpenGL 4.3+，某些系统可能不支持

**缓解**:
- 检测 OpenGL 版本并给出友好提示
- 考虑实现软件渲染后备方案

### 风险 2: API 稳定性
**问题**: libghostty 嵌入 API 尚未稳定

**缓解**:
- 锁定 libghostty 1.3.1 版本
- 跟踪上游 API 变化
- 使用抽象层隔离 API 变化

### 风险 3: 性能问题
**问题**: Qt Quick 和 OpenGL 渲染可能冲突

**缓解**:
- 使用 Qt Quick Scene Graph 的自定义节点
- 确保渲染在正确的线程
- 性能测试和优化

---

## 📚 参考资源

1. **Ghostty 源码**: `/home/xuming/src/ghostty/`
2. **Ghostty C API**: `/home/xuming/src/ghostty/include/ghostty.h`
3. **Ghostty macOS 示例**: `/home/xuming/src/ghostty/macos/Sources/Ghostty/`
4. **Qt Quick Scene Graph**: https://doc.qt.io/qt-6/qtquick-visualcanvas-scenegraph.html
5. **QQuickItem 文档**: https://doc.qt.io/qt-6/qquickitem.html

---

## 📝 下一步行动

1. 验证 libghostty 1.3.1 安装状态
2. 安装 Qt6 和 OpenGL 开发包
3. 创建 Phase 1 详细检查清单
4. 开始实施 Phase 1

**详细检查清单**: 参见 `docs/plans/phase-1-checklist.md`