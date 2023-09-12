echo "Reading logs..."
#PID=`${SDK}/platform-tools/adb shell pidof -s hello_android5`
echo "PID is: ${PID}"
#"${SDK}/platform-tools/adb" logcat --pid="${PID}"
"${SDK}/platform-tools/adb" logcat RustStdoutStderr:D *:S
#"${SDK}/platform-tools/adb" logcat 
