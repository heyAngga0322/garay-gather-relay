# Browser Test Environment Fix

## Problem

Playwright browser tests fail due to `libsimdjson` version mismatch:
- Node.js is linked against `libsimdjson.so.36`
- System has `libsimdjson.so.37`

Error:
```
node: error while loading shared libraries: libsimdjson.so.36: cannot open shared object file: No such file or directory
```

## Root Cause

The system package manager updated `libsimdjson` to v37, but the Node.js binary (installed via nvm/volta or system package) was built against v36.

## Solutions

### Option 1: Symlink Workaround (Quick Fix)

```bash
# Find the actual library
find /usr -name "libsimdjson.so*" 2>/dev/null

# Create symlink from v37 to v36
sudo ln -sf /usr/lib/libsimdjson.so.37 /usr/lib/libsimdjson.so.36

# Verify
ldd $(which node) | grep simdjson
```

**Note**: This is a temporary fix. May break if API changes between versions.

### Option 2: Pin Node.js Version (Recommended)

Use a Node version manager to pin a compatible version:

#### Using nvm

```bash
# Install nvm if not present
curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.39.7/install.sh | bash
source ~/.bashrc

# Install and use a specific Node version known to work
nvm install 20.12.0
nvm use 20.12.0
nvm alias default 20.12.0

# Reinstall Playwright
npm ci
npx playwright install chromium
```

#### Using volta (Alternative)

```bash
# Install volta
curl https://get.volta.sh | bash
source ~/.bashrc

# Pin Node version
volta install node@20.12.0
volta pin node@20.12.0

# Reinstall
npm ci
npx playwright install chromium
```

### Option 3: Use System Node with Compatible libsimdjson

```bash
# Downgrade libsimdjson (Arch/CachyOS)
sudo pacman -U https://archive.archlinux.org/packages/l/libsimdjson/libsimdjson-3.6.0-1-x86_64.pkg.tar.zst

# Or pin in pacman.conf
# Add to /etc/pacman.conf:
# IgnorePkg = libsimdjson
```

### Option 4: Docker-based Test Environment (CI/CD Ready)

Create a Dockerfile for consistent test environment:

```dockerfile
# Dockerfile.playwright
FROM mcr.microsoft.com/playwright:v1.45.0-noble

WORKDIR /app

# Copy package files
COPY package*.json ./
RUN npm ci

# Copy test files
COPY tests/ ./tests/
COPY playwright.config.js ./

# Run tests
CMD ["npm", "run", "test:browser", "--", "--reporter=list"]
```

```bash
# Build and run
docker build -f Dockerfile.playwright -t garay-playwright .
docker run --rm --network host garay-playwright
```

**For CI/CD** (GitHub Actions):

```yaml
# .github/workflows/browser-tests.yml
name: Browser Tests

on: [push, pull_request]

jobs:
  browser-tests:
    runs-on: ubuntu-latest
    container:
      image: mcr.microsoft.com/playwright:v1.45.0-noble
    steps:
      - uses: actions/checkout@v4
      - name: Install dependencies
        run: npm ci
      - name: Run browser tests
        run: npm run test:browser -- --reporter=list
```

### Option 5: Install libsimdjson36 from AUR (Arch-based)

```bash
# Using yay/paru
yay -S libsimdjson36

# Or build from source
git clone https://aur.archlinux.org/libsimdjson36.git
cd libsimdjson36
makepkg -si
```

## Verification

After applying a fix, verify tests pass:

```bash
# Start server in background
cargo run &
SERVER_PID=$!
sleep 3

# Run tests
npm run test:browser -- --reporter=list
TEST_RESULT=$?

# Cleanup
kill $SERVER_PID

exit $TEST_RESULT
```

## Prevention

1. **Pin Node version** in `.nvmrc` or `package.json`:
   ```json
   {
     "engines": { "node": "20.12.0" }
   }
   ```

2. **Use Docker for CI** - eliminates host dependency issues

3. **Add pre-test check** to `package.json`:
   ```json
   {
     "scripts": {
       "pretest:browser": "ldd $(which node) | grep -q simdjson && echo 'OK' || (echo 'libsimdjson mismatch!' && exit 1)",
       "test:browser": "playwright test"
     }
   }
   ```

4. **Document in README** for new contributors