# libghostty 集成调试日志 - 段错误问题

**日期**: 2026-03-22
**状态**: 问题排查中
**优先级**: 高

## 问题描述

尝试使用 Python (cffi 和 ctypes) 加载 `/usr/local/lib/libghostty.so` 时，程序立即崩溃并产生段错误（Segmentation Fault）。

## 尝试的方法

### 1. 使用 cffi 加载
```python
from cffi import FFI
ffi = FFI()
lib = ffi.dlopen("/usr/local/lib/libghostty.so")  # 段错误
```

**结果**: 段错误 (Exit code 139)

### 2. 使用 ctypes 加载
```python
import ctypes
lib = ctypes.CDLL("/usr/local/lib/libghostty.so")  # 段错误
```

**结果**: 段错误 (Exit code 139)

## 库信息

### 文件属性
```
/usr/local/lib/libghostty.so: ELF 64-bit LSB shared object, ARM aarch64, version 1 (SYSV), dynamically linked, stripped
```

### 依赖
```
linux-vdso.so.1
libm.so.6 => /usr/lib/aarch64-linux-gnu/libm.so.6
libc.so.6 => /usr/lib/aarch64-linux-gnu/libc.so.6
/lib/ld-linux-aarch64.so.1
```

### 架构
- 架构: ARM aarch64
- OS/ABI: UNIX - System V
- 动态链接，已 strip

## 可能的原因

### 1. 全局构造函数问题
libghostty 可能包含全局构造函数（`__attribute__((constructor))`），在库加载时执行代码，而这些代码可能：
- 需要 OpenGL 上下文
- 需要特定的环境变量
- 需要某些系统服务

### 2. 线程本地存储 (TLS) 问题
某些库在加载时初始化线程本地存储，可能导致问题。

### 3. 缺少依赖
虽然 `ldd` 显示依赖都满足，但可能缺少运行时依赖（如图形服务器连接）。

### 4. ABI 不兼容
库可能是用不同的编译器选项或标准库版本编译的。

## 下一步调试步骤

### A. 使用 GDB 调试
```bash
gdb python
(gdb) run tests/test_load_library.py
(gdb) bt  # 查看堆栈跟踪
```

### B. 使用 LD_DEBUG
```bash
LD_DEBUG=all python tests/test_load_library.py 2>&1 | head -100
```

### C. 检查 ghostty 源码中的全局初始化
查看 `/home/xuming/src/ghostty/src/` 中的全局构造函数：
```bash
grep -r "__attribute__((constructor))" /home/xuming/src/ghostty/src/
grep -r "export fn _start" /home/xuming/src/ghostty/src/
```

### D. 尝试加载 libghostty-vt.so
libghostty-vt 是更小的库，可能没有渲染相关的初始化：
```bash
python -c "import ctypes; ctypes.CDLL('/usr/local/lib/libghostty-vt.so')"
```

### E. 检查 ghostty 文档
查看 ghostty 是否有关于嵌入的特殊说明：
- `/home/xuming/src/ghostty/README.md`
- `/home/xuming/src/ghostty/HACKING.md`
- `/home/xuming/src/ghostty/include/ghostty.h` 头文件注释

### F. 联系 ghostty 社区
如果以上方法都无法解决，考虑：
1. 在 ghostty GitHub issues 搜索类似问题
2. 提交新的 issue 询问 Python 嵌入支持

## 替代方案

### 方案 1: 使用 libghostty-vt
如果 libghostty-vt 可以加载，可以：
- 使用 libghostty-vt 处理终端仿真
- 自己实现 OpenGL 渲染

### 方案 2: 子进程方式
如果直接嵌入不可行，可以：
- 将 ghostty 作为独立进程运行
- 通过 IPC/共享内存进行通信

### 方案 3: 重新编译 libghostty
检查是否可以在编译时禁用某些初始化：
```bash
cd /home/xuming/src/ghostty
zig build -Dapp-runtime=embedded -Denable-gpu=false
```

## 文档更新

- [ ] 记录 GDB 调试结果
- [ ] 记录 ghostty 源码中的全局初始化发现
- [ ] 更新计划文档以反映此问题
- [ ] 如果解决，创建详细的集成指南

## 相关文件

- 测试脚本: `tests/test_load_library.py`
- 测试脚本: `tests/test_ctypes.py`
- 绑定代码: `src/witty/ghostty/bindings.py`
- ghostty 源码: `/home/xuming/src/ghostty/`
- ghostty 头文件: `/usr/local/include/ghostty.h`

---

**优先级**: 必须在继续 Phase 1 之前解决此问题。