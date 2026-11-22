#!/bin/sh
# 本地交叉编译 natfrpctl（Linux / WSL；用官方 Rust 工具链）
#
#   ./scripts/build.sh              # 默认 aarch64
#   ARCH=x86_64 ./scripts/build.sh
#
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
crate="$root/src/natfrpctl/src"
out="$root/src/natfrpctl/files"
arch=${ARCH:-aarch64}

case "$arch" in
	aarch64) target=aarch64-unknown-linux-musl ;;
	x86_64)  target=x86_64-unknown-linux-musl ;;
	arm)     target=armv7-unknown-linux-musleabihf ;;
	i386)    target=i686-unknown-linux-musl ;;
	*) echo "不支持的架构: $arch" >&2; exit 1 ;;
esac

mkdir -p "$out"
cd "$crate"

rustup target add "$target" >/dev/null 2>&1 || true

RUSTFLAGS="-C link-self-contained=yes -C linker=rust-lld" \
	cargo build --release --locked --target "$target"

cp "target/$target/release/natfrpctl" "$out/natfrpctl-$arch"
strip "$out/natfrpctl-$arch" 2>/dev/null || true

echo "已生成: $out/natfrpctl-$arch"
ls -l "$out/natfrpctl-$arch"
