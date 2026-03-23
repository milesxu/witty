# 重新编译 libghostty Debug 版本

**日期**: 2026-03-22
**目的**: 使用 debug 模式重新编译 libghostty，便于调试和定位段错误问题

## 编译环境

- **Zig 版本**: 0.15.2
- **Ghostty 源码**: `/home/xuming/src/ghostty/`
- **目标**: Linux aarch64

## 编译选项

根据 `zig build --help` 输出：

### 优化选项
```
-Doptimize=Debug     # Debug 模式，包含调试符号
```

### App Runtime 选项
```
-Dapp-runtime=none   # 嵌入模式，不包含 GTK/macOS UI
```

## 编译步骤

### 1. 编译 libghostty (完整库，嵌入模式)

```bash
cd /home/xuming/src/ghostty
zig build -Doptimize=Debug -Dapp-runtime=none
```

**输出位置**: `zig-out/lib/libghostty.so`

### 2. 编译 libghostty-vt (仅终端仿真库)

```bash
zig build lib-vt -Doptimize=Debug
```

**输出位置**: `zig-out/lib/libghostty-vt.so`

## 预期优势

### Debug 模式的好处
1. **调试符号**: 包含完整的调试信息
2. **未优化**: 代码未优化，更容易调试
3. **安全检查**: 启用运行时安全检查
4. **更好的错误信息**: 更详细的错误报告

### 嵌入模式 (-Dapp-runtime=none)
1. **无 UI 依赖**: 不依赖 GTK 或其他 UI 框架
2. **更小的库**: 去除 UI 相关代码
3. **更简单的初始化**: 可能避免复杂的 UI 初始化

## 验证编译结果

### 检查库文件
```bash
ls -lah zig-out/lib/libghostty*.so
file zig-out/lib/libghostty.so
```

### 检查调试符号
```bash
readelf -S zig-out/lib/libghostty.so | grep debug
nm zig-out/lib/libghostty.so | grep ghostty_info
```

### 测试加载
```bash
# 测试新编译的库
python tests/test_load_library.py
# 或
python tests/test_ghostty_vt.py
```

## 安装选项

### 临时使用（推荐）
直接使用 `zig-out/lib/` 中的库，不安装到系统：

```bash
export LD_LIBRARY_PATH=/home/xuming/src/ghostty/zig-out/lib:$LD_LIBRARY_PATH
```

### 安装到系统（可选）
```bash
sudo cp zig-out/lib/libghostty.so /usr/local/lib/libghostty-debug.so
sudo ldconfig
```

## 下一步

### 如果 debug 版本可以加载
1. 使用 GDB 详细调试
2. 检查初始化流程
3. 识别导致段错误的具体位置

### 如果 debug 版本仍然段错误
1. 使用 GDB 单步调试初始化代码
2. 检查 Zig 的全局构造函数
3. 考虑使用 C++ 中间层 (PyBind11)

### 如果嵌入模式 (-Dapp-runtime=none) 可行
1. 这是最佳方案
2. 继续使用此版本进行开发
3. 更新文档和集成计划

## PyBind11 准备

如果需要 C++ 中间层，已准备好：

### 安装 PyBind11
```bash
uv add pybind11
```

### C++ 包装器结构
```
src/witty/ghostty_cpp/
├── CMakeLists.txt
├── ghostty_wrapper.cpp
└── ghostty_wrapper.hpp
```

### 基本结构
```cpp
#include <pybind11/pybind11.h>
#include <ghostty.h>

namespace py = pybind11;

class GhosttyApp {
public:
    GhosttyApp() {
        // 初始化 ghostty
    }
    // ...
};

PYBIND11_MODULE(witty_ghostty, m) {
    py::class_<GhosttyApp>(m, "GhosttyApp")
        .def(py::init<>())
        .def("tick", &GhosttyApp::tick);
}
```

## 参考文档

- Ghostty 构建文档: `/home/xuming/src/ghostty/HACKING.md`
- Zig 构建系统: `zig build --help`
- PyBind11 文档: https://pybind11.readthedocs.io/

---

**状态**: 准备执行编译