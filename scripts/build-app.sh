#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
cargo build --release --locked
app="dist/Dubplate Devices.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/dubplate-devices "$app/Contents/MacOS/dubplate-devices.new"
mv "$app/Contents/MacOS/dubplate-devices.new" "$app/Contents/MacOS/dubplate-devices"
cp resources/*.jigbp "$app/Contents/Resources/"
cp resources/Info.plist "$app/Contents/Info.plist"
codesign --force --sign - "$app"
printf 'Built: %s/%s\n' "$PWD" "$app"
