# Phase 1 检查清单：环境准备和基础集成

**开始日期**: 2026-03-22
**预计完成**: Week 1-2
**状态**: 进行中

---

## ✅ 任务检查表

### 1. 依赖安装和验证

#### 1.1 libghostty 验证
- [ ] 检查头文件存在
  ```bash
  ls /usr/include/ghostty.h
  ```
- [ ] 检查库文件存在
  ```bash
  ls /usr/lib/libghostty.so
  ls /usr/lib/libghostty.a
  ```
- [ ] 验证 pkg-config
  ```bash
  pkg-config --libs --cflags ghostty
  ```
- [ ] 检查版本
  ```bash
  pkg-config --modversion ghostty
  ```

**备注**:
- 已知 libghostty 1.3.1 已安装到系统
- 需要确认是否为共享库 (.so) 或静态库 (.a)

#### 1.2 Qt6 开发包安装
- [ ] 安装基础包
  ```bash
  sudo apt install qt6-base-dev qt6-declarative-dev
  ```
- [ ] 安装 OpenGL 支持
  ```bash
  sudo apt install qt6-opengl-dev libgl1-mesa-dev libglu1-mesa-dev
  ```
- [ ] 验证 Qt6 安装
  ```bash
  qmake6 --version
  ```
- [ ] 验证 OpenGL
  ```bash
  glxinfo | grep "OpenGL version"
  ```

#### 1.3 构建工具安装
- [ ] 安装 CMake
  ```bash
  cmake --version  # 需要 >= 3.16
  ```
- [ ] 安装编译器
  ```bash
  g++ --version  # 需要 >= 11
  ```

---

### 2. 项目配置

#### 2.1 创建 C++ 目录结构
- [ ] 创建 `src/witty/ghostty/` 目录
  ```bash
  mkdir -p src/witty/ghostty
  ```
- [ ] 创建测试目录
  ```bash
  mkdir -p tests/cpp
  ```

#### 2.2 创建 CMakeLists.txt

**主 CMakeLists.txt** (`CMakeLists.txt`):
- [ ] 设置 C++ 标准 (C++17)
- [ ] 查找 Qt6 包
- [ ] 查找 libghostty
- [ ] 添加子目录

**子目录 CMakeLists.txt** (`src/witty/ghostty/CMakeLists.txt`):
- [ ] 定义 GhosttyLibrary 目标
- [ ] 链接 Qt6::Quick, Qt6::OpenGL
- [ ] 链接 libghostty
- [ ] 设置包含路径

#### 2.3 更新 pyproject.toml
- [ ] 添加 CMake 构建脚本
  ```toml
  [tool.hatch.build.targets.wheel.hooks.custom]
  path = "scripts/build_cpp.py"
  ```

---

### 3. 创建 C++ 绑定层

#### 3.1 GhosttyApp 类

**文件**: `src/witty/ghostty/ghostty_app.h`
- [ ] 定义类声明
  ```cpp
  namespace witty::ghostty {
  class GhosttyApp : public QObject {
      Q_OBJECT
  public:
      explicit GhosttyApp(QObject* parent = nullptr);
      ~GhosttyApp();

      bool initialize();
      void tick();

  private:
      ghostty_app_t app_{nullptr};
      ghostty_config_t config_{nullptr};
  };
  }
  ```

**文件**: `src/witty/ghostty/ghostty_app.cpp`
- [ ] 实现构造函数
- [ ] 实现初始化逻辑
  - `ghostty_init()`
  - `ghostty_config_new()`
  - `ghostty_app_new()`
- [ ] 实现析构函数
  - `ghostty_app_free()`
  - `ghostty_config_free()`
- [ ] 实现 `tick()`
  - `ghostty_app_tick()`

#### 3.2 GhosttySurface 类

**文件**: `src/witty/ghostty/ghostty_surface.h`
- [ ] 定义类声明
  ```cpp
  namespace witty::ghostty {
  class GhosttySurface {
  public:
      explicit GhosttySurface(ghostty_app_t app);
      ~GhosttySurface();

      bool initialize();
      void setSize(uint32_t width, uint32_t height);
      void draw();

  private:
      ghostty_surface_t surface_{nullptr};
  };
  }
  ```

**文件**: `src/witty/ghostty/ghostty_surface.cpp`
- [ ] 实现构造函数
- [ ] 实现初始化
  - `ghostty_surface_new()`
- [ ] 实现析构函数
  - `ghostty_surface_free()`
- [ ] 实现 `setSize()`
  - `ghostty_surface_set_size()`
- [ ] 实现 `draw()`
  - `ghostty_surface_draw()`

#### 3.3 基础测试

**文件**: `tests/cpp/test_ghostty_app.cpp`
- [ ] 测试 GhosttyApp 初始化
- [ ] 测试生命周期管理

**文件**: `tests/cpp/test_ghostty_surface.cpp`
- [ ] 测试 Surface 创建
- [ ] 测试尺寸设置

---

### 4. 编译和验证

#### 4.1 配置 CMake
- [ ] 创建 build 目录
  ```bash
  mkdir -p build && cd build
  ```
- [ ] 运行 CMake 配置
  ```bash
  cmake .. -DCMAKE_BUILD_TYPE=Debug
  ```
- [ ] 检查配置输出
  - 确认找到 Qt6
  - 确认找到 libghostty

#### 4.2 编译
- [ ] 编译项目
  ```bash
  cmake --build .
  ```
- [ ] 解决编译错误
- [ ] 验证生成的库文件
  ```bash
  ls src/witty/ghostty/libwitty_ghostty.a
  ```

#### 4.3 运行测试
- [ ] 运行单元测试
  ```bash
  ctest --output-on-failure
  ```
- [ ] 验证测试通过

---

### 5. 文档更新

#### 5.1 创建集成文档
- [ ] 创建 `docs/integration/setup.md`
  - 依赖安装指南
  - 环境配置
  - 常见问题

#### 5.2 创建构建文档
- [ ] 创建 `docs/integration/building.md`
  - CMake 配置说明
  - 编译步骤
  - 安装步骤

#### 5.3 更新 README
- [ ] 添加构建说明
- [ ] 添加依赖要求

---

## 🐛 已知问题

### Issue #1: libghostty 位置
**问题**: 不确定 libghostty 安装在哪个路径

**解决**: 运行 `find /usr -name "libghostty*" 2>/dev/null` 查找

**状态**: 待解决

### Issue #2: Qt6 版本
**问题**: PySide6 6.7.3 捆绑的 Qt6 版本可能影响编译

**解决**: 需要确认 Qt6 开发包版本与 PySide6 一致

**状态**: 待确认

---

## 📝 进度日志

### 2026-03-22
- [x] 创建 Phase 1 检查清单
- [ ] 开始依赖验证

---

## 🎯 里程碑

- **M1**: 所有依赖安装完成
- **M2**: 项目可编译
- **M3**: 单元测试通过
- **M4**: Phase 1 完成

---

## 📚 参考文档

- [Qt6 CMake 集成](https://doc.qt.io/qt-6/cmake-manual.html)
- [libghostty API](../technical/libghostty-analysis.md)
- [总体计划](qt6-integration-plan.md)