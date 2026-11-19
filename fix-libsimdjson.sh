#!/bin/bash
# fix-libsimdjson.sh - Fix libsimdjson version mismatch for Playwright tests
# Run with: bash fix-libsimdjson.sh

set -euo pipefail

echo "🔧 Fixing libsimdjson version mismatch for Playwright..."

# Check current Node.js libsimdjson dependency
NODE_PATH=$(which node)
echo "Node.js path: $NODE_PATH"

if ! ldd "$NODE_PATH" 2>/dev/null | grep -q simdjson; then
    echo "✅ Node.js doesn't depend on libsimdjson (static build or different version)"
    exit 0
fi

echo "Current libsimdjson dependency:"
ldd "$NODE_PATH" | grep simdjson

# Find available libsimdjson versions
echo ""
echo "Available libsimdjson libraries:"
find /usr -name "libsimdjson.so*" 2>/dev/null | sort -V

# Try to find libsimdjson.so.36
TARGET_LIB="/usr/lib/libsimdjson.so.36"
SOURCE_LIB=""

if [[ -f "$TARGET_LIB" ]]; then
    echo "✅ libsimdjson.so.36 already exists"
    exit 0
fi

# Find the newest available version
for lib in /usr/lib/libsimdjson.so.*; do
    if [[ -f "$lib" ]]; then
        SOURCE_LIB="$lib"
    fi
done

if [[ -z "$SOURCE_LIB" ]]; then
    echo "❌ No libsimdjson found in /usr/lib"
    echo "Try installing: sudo pacman -S libsimdjson (or your distro's equivalent)"
    exit 1
fi

echo ""
echo "Found: $SOURCE_LIB"
echo "Creating symlink: $TARGET_LIB -> $SOURCE_LIB"

# Create symlink (requires sudo)
if [[ $EUID -eq 0 ]]; then
    ln -sf "$SOURCE_LIB" "$TARGET_LIB"
else
    sudo ln -sf "$SOURCE_LIB" "$TARGET_LIB"
fi

# Verify
echo ""
echo "Verification:"
ldd "$NODE_PATH" | grep simdjson

# Test Playwright
echo ""
echo "Testing Playwright..."
if command -v npx &> /dev/null; then
    npx playwright --version
    echo "✅ Playwright works"
else
    echo "⚠️  npx not found, skipping Playwright test"
fi

echo ""
echo "✅ Fix applied successfully!"
echo "Run tests with: npm run test:browser -- --reporter=list"