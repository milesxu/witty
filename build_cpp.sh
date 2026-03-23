#!/bin/bash

set -e

echo "========================================"
echo "Building Witty Terminal (C++)"
echo "========================================"

# Set Qt path
export Qt6_DIR="/home/xuming/src/witty/6.7.3/gcc_arm64/lib/cmake/Qt6"
export PATH="/home/xuming/src/witty/6.7.3/gcc_arm64/bin:$PATH"

# Create build directory
BUILD_DIR="build_cpp"
mkdir -p "$BUILD_DIR"
cd "$BUILD_DIR"

# Configure
echo ""
echo "Configuring..."
cmake ../src/cpp \
    -DCMAKE_BUILD_TYPE=Debug \
    -DCMAKE_PREFIX_PATH="/home/xuming/src/witty/6.7.3/gcc_arm64/lib/cmake"

# Build
echo ""
echo "Building..."
cmake --build . --parallel $(nproc)

echo ""
echo "========================================"
echo "✓ Build completed!"
echo "========================================"
echo ""
echo "Run with: ./$BUILD_DIR/witty-terminal"