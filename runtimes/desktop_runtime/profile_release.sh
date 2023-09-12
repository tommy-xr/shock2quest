#!/usr/bin/env bash

# Creates a build for profiling on OSX- release, with entitl

echo "Building release"
cargo build --release

echo "Creating entitlements file..."
/usr/libexec/PlistBuddy -c "Add :com.apple.security.get-task-allow bool true" tmp.entitlements

echo "Creating entitlements..."
codesign -s - --entitlements tmp.entitlements -f ../target/release/desktop_runtime

echo "Running release build"
../target/release/desktop_runtime -m=medsci2.mis -e=gui
