export SDK="/opt/homebrew/Caskroom/android-sdk/4333796/"
export ANDROID_SDK_ROOT="${SDK}"
export BUILD_TOOLS="${SDK}/build-tools/33.0.0"
export PLATFORM="${SDK}/platforms/android-26"
export JAVA_HOME="/Applications/Android Studio.app/Contents/jre/Contents/Home"
export NDK="${SDK}/ndk/24.0.8215888"
export ANDROID_NDK_ROOT="${NDK}"
export ARM64_TOOLCHAIN="${NDK}/toolchains/llvm/prebuilt/darwin-x86_64"
export CC="${ARM64_TOOLCHAIN}/bin/aarch64-linux-android26-clang"
export CCP="${ARM64_TOOLCHAIN}/bin/aarch64-linux-android26-clang++"
export SYSROOT="${ARM64_TOOLCHAIN}/sysroot"

# Set target-specific versions for cargo (don't set global CFLAGS to avoid affecting host compilation)
export CC_aarch64_linux_android="${CC}"
export CXX_aarch64_linux_android="${CCP}"
export CFLAGS_aarch64_linux_android="--sysroot=${SYSROOT} -fPIC"
export CXXFLAGS_aarch64_linux_android="--sysroot=${SYSROOT} -fPIC"

# Set host compiler flags to include ffmpeg headers (without Android sysroot)
export CFLAGS_aarch64_apple_darwin="-I${FFMPEG_ROOT}/include"
export CXXFLAGS_aarch64_apple_darwin="-I${FFMPEG_ROOT}/include"

# Unset global CFLAGS that might interfere with host compilation
unset CFLAGS
unset CXXFLAGS
unset CPPFLAGS
unset LDFLAGS

# Set environment variables for bindgen to find our headers
export BINDGEN_EXTRA_CLANG_ARGS="-I${FFMPEG_ROOT}/include --sysroot=${SYSROOT} --target=aarch64-linux-android"

# Set the specific environment variable that ffmpeg-sys-next looks for
export FFMPEG_INCLUDE_DIR="${FFMPEG_ROOT}/include"

# Also set CLANG_PATH to ensure bindgen uses the right compiler
export LIBCLANG_PATH="/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/lib"

# Force HOST_CC to use system clang to avoid cross-compilation issues with build scripts
export HOST_CC="clang"

# Also set the target-specific host CC
export CC_aarch64_apple_darwin="clang"

# Try to force static linking to avoid runtime dependency issues
export FFMPEG_STATIC="1"

# Unset CC temporarily during cargo build to let it use default host compiler for build scripts
alias cargo_android='unset CC && cargo'
"${CC}" --version
"${CCP}" --version

export PATH="$PATH:${SDK}/platform-tools"

# FFmpeg cross-compilation setup
export FFMPEG_ROOT="$(pwd)/ffmpeg"
export PKG_CONFIG_ALLOW_CROSS=1
export PKG_CONFIG_PATH="${FFMPEG_ROOT}/pkgconfig"
export PKG_CONFIG_SYSROOT_DIR="${FFMPEG_ROOT}"

# Set target-specific pkg-config variables
export PKG_CONFIG_ALLOW_CROSS_aarch64_linux_android=1
export PKG_CONFIG_PATH_aarch64_linux_android="${FFMPEG_ROOT}/pkgconfig"
export PKG_CONFIG_SYSROOT_DIR_aarch64_linux_android="${FFMPEG_ROOT}"

echo "FFmpeg cross-compilation environment configured:"
echo "  FFMPEG_ROOT: ${FFMPEG_ROOT}"
echo "  PKG_CONFIG_PATH: ${PKG_CONFIG_PATH}"
echo "  CC: ${CC}"
echo "  HOST_CC: ${HOST_CC}"
echo "  CC_aarch64_apple_darwin: ${CC_aarch64_apple_darwin}"
