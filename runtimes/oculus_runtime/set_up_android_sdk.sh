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
"${CC}" --version
"${CCP}" --version

export PATH="$PATH:${SDK}/platform-tools"
