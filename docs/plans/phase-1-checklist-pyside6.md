# Phase 1 检查清单：环境准备和基础集成（Python/PySide6 方案）

**开始日期**: 2026-03-22
**预计完成**: Week 1-2
**状态**: 进行中

---

## ✅ 任务检查表

### 1. 依赖验证和安装

#### 1.1 已安装依赖验证
- [x] Python 3.14.3 已安装
- [x] PySide6 6.7.3 已安装
- [x] cffi 已安装（用于调用 C API）

#### 1.2 libghostty 验证
- [ ] 查找 libghostty 库文件
  ```bash
  find /usr -name "libghostty*" 2>/dev/null
  ```
- [ ] 检查共享库
  ```bash
  ls -la /usr/lib/libghostty.so
  ```
- [ ] 检查头文件（可选，用于参考）
  ```bash
  ls -la /usr/include/ghostty.h
  ```
- [ ] 测试库是否可加载
  ```python
  from cffi import FFI
  ffi = FFI()
  lib = ffi.dlopen("libghostty.so")  # 测试加载
  ```

#### 1.3 PySide6 OpenGL 支持验证
- [ ] 验证 PySide6 OpenGL 模块
  ```python
  from PySide6.QtOpenGL import QOpenGLContext, QOpenGLFunctions
  ```
- [ ] 测试 OpenGL 上下文创建
  ```python
  from PySide6.QtWidgets import QApplication
  from PySide6.QtOpenGL import QOpenGLContext

  app = QApplication([])
  ctx = QOpenGLContext()
  print(ctx.create())  # 应该返回 True
  ```

#### 1.4 额外依赖安装（如需要）
- [ ] 安装 PyOpenGL（如果需要直接 OpenGL 调用）
  ```bash
  uv add PyOpenGL PyOpenGL-accelerate
  ```

---

### 2. 项目结构创建

#### 2.1 创建 ghostty 模块目录
- [ ] 创建 `src/witty/ghostty/` 目录
  ```bash
  mkdir -p src/witty/ghostty
  ```
- [ ] 创建 `__init__.py`
  ```bash
  touch src/witty/ghostty/__init__.py
  ```

#### 2.2 创建绑定层文件
- [ ] 创建 `src/witty/ghostty/bindings.py` - cffi 绑定定义
- [ ] 创建 `src/witty/ghostty/app.py` - GhosttyApp 封装
- [ ] 创建 `src/witty/ghostty/surface.py` - GhosttySurface 封装
- [ ] 创建 `src/witty/ghostty/terminal.py` - QQuickItem 终端组件

---

### 3. Python FFI 绑定层实现

#### 3.1 定义 C API (bindings.py)

**从 ghostty.h 提取关键定义**:
- [ ] 复制 `/home/xuming/src/ghostty/include/ghostty.h` 到 `docs/reference/`
- [ ] 提取并定义核心类型
  ```python
  from cffi import FFI

  ffi = FFI()
  ffi.cdef("""
      // Opaque types
      typedef void* ghostty_app_t;
      typedef void* ghostty_config_t;
      typedef void* ghostty_surface_t;

      // Initialization
      int ghostty_init(uintptr_t, char**);

      // App management
      ghostty_app_t ghostty_app_new(const ghostty_runtime_config_s*, ghostty_config_t);
      void ghostty_app_free(ghostty_app_t);
      void ghostty_app_tick(ghostty_app_t);

      // Surface management
      ghostty_surface_t ghostty_surface_new(ghostty_app_t, const ghostty_surface_config_s*);
      void ghostty_surface_free(ghostty_surface_t);
      void ghostty_surface_draw(ghostty_surface_t);
      void ghostty_surface_set_size(ghostty_surface_t, uint32_t, uint32_t);

      // Config
      ghostty_config_t ghostty_config_new();
      void ghostty_config_free(ghostty_config_t);
      void ghostty_config_load_file(ghostty_config_t, const char*);

      // Input
      bool ghostty_surface_key(ghostty_surface_t, ghostty_input_key_s);
      void ghostty_surface_text(ghostty_surface_t, const char*, uintptr_t);

      // ... 其他必要的 API
  """)
  ```

- [ ] 定义回调结构
  ```python
  ffi.cdef("""
      typedef struct {
          void* userdata;
          bool supports_selection_clipboard;
          void* wakeup_cb;
          void* action_cb;
          void* read_clipboard_cb;
          void* write_clipboard_cb;
          void* close_surface_cb;
      } ghostty_runtime_config_s;
  """)
  ```

- [ ] 加载库
  ```python
  # 尝试多个可能的路径
  lib_paths = [
      "libghostty.so",
      "/usr/lib/libghostty.so",
      "/usr/local/lib/libghostty.so",
  ]

  lib = None
  for path in lib_paths:
      try:
          lib = ffi.dlopen(path)
          print(f"✓ Loaded libghostty from {path}")
          break
      except OSError:
          continue

  if lib is None:
      raise RuntimeError("Cannot find libghostty.so")
  ```

#### 3.2 测试基本 API 调用
- [ ] 创建测试脚本 `tests/test_bindings.py`
  ```python
  def test_ghostty_init():
      result = lib.ghostty_init(0, ffi.NULL)
      assert result == 0

  def test_config_new():
      config = lib.ghostty_config_new()
      assert config != ffi.NULL
      lib.ghostty_config_free(config)
  ```

---

### 4. 封装 Python 类

#### 4.1 GhosttyApp 类 (app.py)

- [ ] 实现类框架
  ```python
  from typing import Optional
  from .bindings import ffi, lib

  class GhosttyApp:
      def __init__(self):
          self._app = None
          self._config = None

      def initialize(self) -> bool:
          # 初始化 ghostty
          result = lib.ghostty_init(0, ffi.NULL)
          if result != 0:
              return False

          # 创建配置
          self._config = lib.ghostty_config_new()

          # 创建 app
          # TODO: 设置 runtime_config
          self._app = lib.ghostty_app_new(ffi.NULL, self._config)

          return True

      def tick(self):
          if self._app:
              lib.ghostty_app_tick(self._app)

      def __del__(self):
          if self._app:
              lib.ghostty_app_free(self._app)
          if self._config:
              lib.ghostty_config_free(self._config)
  ```

- [ ] 编写单元测试

#### 4.2 GhosttySurface 类 (surface.py)

- [ ] 实现类框架
  ```python
  class GhosttySurface:
      def __init__(self, app: GhosttyApp):
          self._surface = None
          self._app = app

      def create(self) -> bool:
          # 创建 surface
          self._surface = lib.ghostty_surface_new(self._app._app, ffi.NULL)
          return self._surface != ffi.NULL

      def set_size(self, width: int, height: int):
          if self._surface:
              lib.ghostty_surface_set_size(self._surface, width, height)

      def draw(self):
          if self._surface:
              lib.ghostty_surface_draw(self._surface)

      def __del__(self):
          if self._surface:
              lib.ghostty_surface_free(self._surface)
  ```

- [ ] 编写单元测试

---

### 5. PySide6 集成测试

#### 5.1 OpenGL 上下文测试
- [ ] 创建简单的 OpenGL 窗口测试
  ```python
  from PySide6.QtWidgets import QApplication, QOpenGLWidget
  from PySide6.QtOpenGL import QOpenGLFunctions

  class TestWidget(QOpenGLWidget, QOpenGLFunctions):
      def initializeGL(self):
          self.initializeOpenGLFunctions()
          print(f"OpenGL Version: {self.glGetString(0x1F02)}")

  app = QApplication([])
  widget = TestWidget()
  widget.show()
  app.exec()
  ```

#### 5.2 QQuickItem + OpenGL 测试
- [ ] 测试 QQuickItem 的 Scene Graph 访问
  ```python
  from PySide6.QtQuick import QQuickItem
  from PySide6.QtOpenGL import QOpenGLContext

  class TestItem(QQuickItem):
      def updatePaintNode(self, node, data):
          # 测试访问 OpenGL 上下文
          ctx = QOpenGLContext.currentContext()
          if ctx:
              print("✓ Can access OpenGL context in QQuickItem")
          return node
  ```

---

### 6. 文档更新

#### 6.1 创建集成文档
- [ ] 创建 `docs/integration/python-bindings.md`
  - cffi 绑定说明
  - 如何定义 C API
  - 回调函数处理

#### 6.2 创建调试文档
- [ ] 创建 `docs/integration/debugging.md`
  - 常见错误
  - 如何调试 cffi 问题
  - OpenGL 问题排查

---

## 🐛 已知问题

### Issue #1: libghostty 路径不确定
**问题**: 不知道 libghostty.so 具体安装在哪里

**解决**: 使用 `find` 命令查找，或在加载时尝试多个路径

**状态**: 待解决

### Issue #2: OpenGL 版本要求
**问题**: libghostty 需要 OpenGL 4.3+，需要确认系统支持

**解决**: 在测试中检查 OpenGL 版本

**状态**: 待测试

---

## 📝 进度日志

### 2026-03-22
- [x] 创建 Phase 1 检查清单（Python/PySide6 方案）
- [ ] 开始依赖验证

---

## 🎯 里程碑

- **M1**: libghostty 库验证完成
- **M2**: cffi 绑定可用
- **M3**: GhosttyApp/Surface 类实现完成
- **M4**: OpenGL 集成测试通过
- **M5**: Phase 1 完成

---

## 📚 参考文档

- [cffi 文档](https://cffi.readthedocs.io/)
- [PySide6 QtOpenGL](https://doc.qt.io/qtforpython-6/PySide6/QtOpenGL/index.html)
- [libghostty API](../technical/libghostty-analysis.md)
- [总体计划](qt6-integration-plan.md)