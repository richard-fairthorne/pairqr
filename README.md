# pairqr

[![CI](https://github.com/gustawdaniel/pairqr/actions/workflows/ci.yml/badge.svg)](https://github.com/gustawdaniel/pairqr/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/gustawdaniel/pairqr/branch/fix%2Ftarget-device-connect-and-tcp-fallback/graph/badge.svg)](https://app.codecov.io/gh/gustawdaniel/pairqr)

A command-line tool to pair Android devices for wireless ADB debugging by scanning a QR code, just like Android Studio.

**GitHub:** https://github.com/richard-fairthorne/pairqr

![pairqr screenshot](assets/screenshot.png)

## Installation

### macOS (Homebrew)

```bash
brew install richard-fairthorne/tap/pairqr
```

### Windows (Scoop)

```powershell
scoop bucket add richard-fairthorne https://github.com/richard-fairthorne/scoop-bucket
scoop install pairqr
```

### Cargo (Rust)

```bash
cargo install pairqr
```

Or with [cargo-binstall](https://github.com/cargo-bins/cargo-binstall) for faster pre-built binary installation:

```bash
cargo binstall pairqr
```

### Debian/Ubuntu

Download the `.deb` package from [Releases](https://github.com/richard-fairthorne/pairqr/releases):

```bash
sudo dpkg -i pairqr-amd64.deb
```

### Pre-built Binaries

Pre-built binaries for macOS, Linux, and Windows are available on the [Releases](https://github.com/richard-fairthorne/pairqr/releases) page.

**Requirement:** ADB must be installed and in your PATH.

## Usage

1. Enable **Wireless Debugging** in your Android device's Developer Options
2. Run `pairqr`
3. Tap **Pair device with QR code** on your device and scan
