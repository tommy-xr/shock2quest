#![cfg(unix)]

use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::Path,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn unused_local_port() -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind ephemeral test port");
    listener.local_addr().expect("read ephemeral port").port()
}

fn info_is_ready(port: u16) -> bool {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let Ok(mut stream) = TcpStream::connect_timeout(&address, Duration::from_millis(100)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));

    if stream
        .write_all(b"GET /v1/info HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .is_err()
    {
        return false;
    }

    let mut response = String::new();
    stream.read_to_string(&mut response).is_ok() && response.starts_with("HTTP/1.1 200")
}

fn captured_output(child: &mut Child) -> String {
    let mut output = String::new();
    if let Some(stdout) = child.stdout.as_mut() {
        let _ = stdout.read_to_string(&mut output);
    }
    if let Some(stderr) = child.stderr.as_mut() {
        let _ = stderr.read_to_string(&mut output);
    }
    output
}

#[test]
#[ignore = "requires a working GLFW display/window server"]
fn sigterm_stops_the_whole_runtime() {
    let port = unused_local_port();
    let child = Command::new(env!("CARGO_BIN_EXE_debug_runtime"))
        .args(["--mission", "debug_minimal", "--port", &port.to_string()])
        .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("launch debug_runtime");
    let mut child = ChildGuard(child);

    let ready_deadline = Instant::now() + Duration::from_secs(30);
    while !info_is_ready(port) {
        if let Some(status) = child.0.try_wait().expect("poll debug_runtime startup") {
            let output = captured_output(&mut child.0);
            panic!("debug_runtime exited before becoming ready: {status}\n{output}");
        }
        assert!(
            Instant::now() < ready_deadline,
            "debug_runtime did not become ready within 30 seconds"
        );
        thread::sleep(Duration::from_millis(50));
    }

    // SAFETY: `child.id()` names the live child process guarded above, and
    // SIGTERM has no pointer or memory-safety preconditions.
    let result = unsafe { libc::kill(child.0.id() as libc::pid_t, libc::SIGTERM) };
    assert_eq!(result, 0, "send SIGTERM to debug_runtime");

    let exit_deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.0.try_wait().expect("poll debug_runtime exit") {
            assert!(
                status.success(),
                "debug_runtime exited unsuccessfully: {status}"
            );
            break;
        }
        assert!(
            Instant::now() < exit_deadline,
            "debug_runtime kept running after SIGTERM"
        );
        thread::sleep(Duration::from_millis(50));
    }

    TcpListener::bind(("127.0.0.1", port)).expect("SIGTERM should release the HTTP port");
}
