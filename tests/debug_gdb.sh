#!/bin/bash
# Debug script to find where the segfault occurs

echo "Running Python under GDB to trace segfault..."
echo ""

# Create GDB command file
cat > /tmp/gdb_commands.txt <<'EOF'
set pagination off
set logging file /tmp/gdb_output.txt
set logging on
run tests/test_ctypes.py
bt
quit
EOF

cd /home/xuming/src/witty
source .venv/bin/activate

gdb -batch -x /tmp/gdb_commands.txt python

echo ""
echo "GDB output:"
cat /tmp/gdb_output.txt