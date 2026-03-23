# 重大发现：ghostty 嵌入 API 不支持 Linux！

**日期**: 2026-03-22
**状态**: Phase 1 完成但遇到平台限制

---

## 🚨 关键发现

### 从源码分析得出

#### 1. 平台枚举定义

从 `/home/xuming/src/ghostty/include/ghostty.h`：

```c
typedef enum {
  GHOSTTY_PLATFORM_INVALID,
  GHOSTTY_PLATFORM_MACOS,   // ✓ 支持
  GHOSTTY_PLATFORM_IOS,     // ✓ 支持
  // ❌ 没有 GHOSTTY_PLATFORM_LINUX！
} ghostty_platform_e;
```

#### 2. SurfaceConfig 结构体

```c
typedef struct {
  ghostty_platform_e platform_tag;   // 必须设置为 MACOS 或 IOS
  ghostty_platform_u platform;       // 必须提供 NSView 或 UIView
  void* userdata;
  double scale_factor;
  float font_size;
  // ... 其他字段
} ghostty_surface_config_s;
```

#### 3. 平台联合体

```c
typedef struct {
  void* nsview;   // macOS: NSView 指针
} ghostty_platform_macos_s;

typedef struct {
  void* uiview;   // iOS: UIView 指针
} ghostty_platform_ios_s;

typedef union {
  ghostty_platform_macos_s macos;
  ghostty_platform_ios_s ios;
  // ❌ 没有 Linux 选项！
} ghostty_platform_u;
```

---

## 📊 结论

### ❌ **ghostty 嵌入 API 不支持 Linux！**

**原因**：
1. **平台枚举中没有 Linux**
2. **平台联合体中没有 Linux 的 GtkWidget 或 Window**
3. **嵌入 API 只为 macOS/iOS 设计**

---

## 🔍 为什么有 libghostty.so for Linux？

### libghostty 的两个用途

#### 用途 1: GTK Runtime（Linux 主要方式）

```
编译时: -Dapp-runtime=gtk
输出: 独立的 GTK 应用
架构:
  ┌─────────────┐
  │ GTK App     │
  │  (主程序)   │
  └─────────────┘
        ↓
  ┌─────────────┐
  │ libghostty  │
  │  (内部库)   │
  └─────────────┘
```

**特点**：
- ghostty 自己创建 GTK 窗口
- 应用是独立的 GTK 程序
- 不支持嵌入其他应用

#### 用途 2: 嵌入模式（仅 macOS/iOS）

```
编译时: -Dapp-runtime=none
输出: 可嵌入的库
架构:
  ┌─────────────┐
  │ 宿主应用    │
  │ (SwiftUI)   │
  └─────────────┘
        ↓
  ┌─────────────┐
  │ libghostty  │
  │ (嵌入库)    │
  └─────────────┘
```

**特点**：
- 提供 C API 供宿主应用调用
- 在宿主应用窗口中渲染
- **只支持 macOS/iOS**

---

## 💡 我们可以做什么？

### 方案 A: 使用 libghostty-vt ⭐ 推荐

**优点**：
- ✅ 已验证可以成功加载
- ✅ 跨平台（支持 Linux）
- ✅ 完整的终端仿真功能
- ✅ 不依赖平台特定的渲染

**缺点**：
- ❌ 需要自己实现 OpenGL 渲染
- ❌ 需要自己实现字体渲染

**工作量**：2-3 周

**架构**：
```
┌──────────────────────────┐
│ PySide6 Application      │
│  └─ QWidget              │
│      └─ QOpenGLWidget    │  ← 我们创建 OpenGL 上下文
│          └─ 渲染终端     │
└──────────────────────────┘
          ↓
┌──────────────────────────┐
│ libghostty-vt.so         │
│  • VT 解析               │
│  • Terminal 状态管理     │
│  • PTY 通信              │
└──────────────────────────┘
```

---

### 方案 B: 使用 GTK 集成

**优点**：
- ✅ 使用完整的 ghostty 功能
- ✅ 无需自己实现渲染

**缺点**：
- ❌ 需要 GTK 运行时
- ❌ 需要嵌入 GTK widget 到 Qt
- ❌ 技术复杂度高

**工作量**：1-2 周

**架构**：
```
┌──────────────────────────┐
│ PySide6 Application      │
│  └─ QWidget              │
│      └─ GTK Window ID    │  ← 通过 X11/Wayland 嵌入
└──────────────────────────┘
          ↓
┌──────────────────────────┐
│ GTK ghostty App          │
│  • 完整终端功能          │
│  • GTK 渲染              │
└──────────────────────────┘
```

---

### 方案 C: 等待官方 Linux 嵌入支持

**优点**：
- ✅ 理想方案（如果实现）

**缺点**：
- ❌ 时间未知
- ❌ 不确定是否会实现

**工作量**：等待

**相关链接**：
- Ghostty Roadmap: Step 6 "Cross-platform libghostty for Embeddable Terminals" 状态为 "⚠️"（进行中）

---

## 📊 Phase 1 成果总结

### ✅ 成功完成的

1. **找到 libghostty 加载失败的根本原因**
   - 错误的 cffi 结构定义
   - 完整的根本原因分析文档

2. **实现完整回调系统**
   - GhosttyCallbackManager
   - 5 个回调函数
   - PySide6 集成

3. **GhosttyApp 成功初始化**
   - App 层完全可用
   - 回调系统工作正常

4. **完整的文档体系**
   - 技术分析
   - 根本原因分析
   - Surface 与 OpenGL Widget 关系
   - Phase 1 总结

### ❌ 遇到的限制

**平台限制**：ghostty 嵌入 API 不支持 Linux

---

## 🎯 推荐方案

### 立即行动：使用 libghostty-vt

**理由**：
1. ✅ 已验证可加载
2. ✅ 跨平台支持
3. ✅ 完整的终端仿真功能
4. ✅ 清晰的实现路径

**实施步骤**：
1. 加载 libghostty-vt.so
2. 实现 Terminal 类封装
3. 创建 QOpenGLWidget
4. 实现基本的 OpenGL 渲染
5. 实现字体渲染（使用 freetype-py）
6. 集成输入处理

**预期时间**：2-3 周

---

## 📚 相关文档

- Ghostty Roadmap: `/home/xuming/src/ghostty/README.md`
- GTK Runtime: `/home/xuming/src/ghostty/src/apprt/gtk.zig`
- 嵌入 API: `/home/xuming/src/ghostty/include/ghostty.h`

---

**最终结论**：

虽然 ghostty 的完整嵌入 API 在 Linux 上不可用，但我们有可行的替代方案（libghostty-vt），可以继续实现 Witty Terminal。

**Phase 1 核心目标已达成**：
- ✅ 理解了 ghostty 架构
- ✅ 成功加载了库
- ✅ 实现了完整回调
- ✅ App 层工作正常

**下一步**：转向 libghostty-vt 方案