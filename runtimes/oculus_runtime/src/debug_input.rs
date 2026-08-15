//! On-device remote input, so an agent driving the headset over `adb` can aim
//! the hands, hold the trigger, and fire discrete `InputAction`s without a human
//! wearing it.
//!
//! It speaks the same channel vocabulary as the debug runtime
//! (`shock2vr::input::remote`), but as an **override**: the frame loop still
//! builds its `InputContext` from OpenXR every frame and this layers the claimed
//! channels on top, so unclaimed channels keep their live controller values and
//! a human can share the session with the agent.
//!
//! Gated on a config file (env vars do not reach an Android app):
//! `/sdcard/shock2quest/debug-port.txt` holding a port number. Absent or
//! unparseable => nothing is started. The listener binds loopback only, so it is
//! reachable exclusively through `adb forward tcp:<port> tcp:<port>`.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::{fs, thread};

use serde_json::{Value, json};
use shock2vr::input::remote::InputOverrides;
use shock2vr::input::{InputAction, InputActionState};
use shock2vr::input_context::InputContext;

pub const PORT_CONFIG_PATH: &str = "/sdcard/shock2quest/debug-port.txt";

/// State shared between the HTTP thread and the frame loop.
#[derive(Default)]
struct SharedState {
    overrides: InputOverrides,
    /// Actions posted since the last frame, delivered exactly once (they are
    /// edge-triggered, unlike the level-held channels).
    pending_actions: Vec<InputAction>,
    frame: u64,
}

pub struct DebugInputServer {
    state: Arc<Mutex<SharedState>>,
}

impl DebugInputServer {
    /// Start the server if the port config file is present. Returns `None`
    /// otherwise, which is the shipping configuration.
    pub fn start_if_configured(mission: &str) -> Option<DebugInputServer> {
        let port = parse_port(&fs::read_to_string(PORT_CONFIG_PATH).ok()?)?;
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let listener = match TcpListener::bind(addr) {
            Ok(listener) => listener,
            Err(error) => {
                println!("SHOCK2QUEST_DEBUG_SERVER port={port} bind_error={error}");
                return None;
            }
        };
        println!("SHOCK2QUEST_DEBUG_SERVER port={port} mission={mission} status=listening");

        let server = DebugInputServer {
            state: Arc::new(Mutex::new(SharedState::default())),
        };
        let state = Arc::clone(&server.state);
        let mission = mission.to_owned();
        thread::spawn(move || {
            // Requests are tiny and agent-driven, so connections are served one
            // at a time - no runtime, no thread pool.
            for stream in listener.incoming() {
                match stream {
                    Ok(stream) => serve_connection(stream, &state, &mission),
                    Err(error) => println!("SHOCK2QUEST_DEBUG_SERVER accept_error={error}"),
                }
            }
        });
        Some(server)
    }

    /// Layer the claimed channels over this frame's OpenXR-built input, and
    /// deliver any queued discrete actions. Call once per frame, immediately
    /// before `game.update`.
    pub fn apply(&self, input: &mut InputContext, actions: &mut InputActionState) {
        let mut state = self.state.lock().unwrap();
        state.frame += 1;
        state.overrides.apply(input);
        for action in state.pending_actions.drain(..) {
            actions.trigger(action);
        }
    }
}

/// Parse the debug-port config file: a single port number. Port 0 is rejected
/// (it would bind an arbitrary port the host could not forward to).
fn parse_port(raw: &str) -> Option<u16> {
    match raw.trim().parse::<u16>() {
        Ok(port) if port > 0 => Some(port),
        _ => {
            println!(
                "SHOCK2QUEST_DEBUG_SERVER invalid_port={:?} file={PORT_CONFIG_PATH}",
                raw.trim()
            );
            None
        }
    }
}

fn serve_connection(mut stream: TcpStream, state: &Arc<Mutex<SharedState>>, mission: &str) {
    let (status, body) = match read_request(&mut stream) {
        Ok((method, path, body)) => handle_request(state, mission, &method, &path, &body),
        Err(message) => (400, json!({ "error": message })),
    };
    let body = body.to_string();
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n{body}",
        reason = if status == 200 { "OK" } else { "Bad Request" },
        len = body.len(),
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

/// Minimal HTTP/1.1 request read: request line, headers (only `Content-Length`
/// matters), then exactly that many body bytes.
fn read_request(stream: &mut TcpStream) -> Result<(String, String, String), String> {
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .map_err(|error| format!("failed to read request: {error}"))?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_owned();
    let path = parts.next().unwrap_or_default().to_owned();
    if method.is_empty() || path.is_empty() {
        return Err("malformed request line".to_owned());
    }

    let mut content_length = 0usize;
    loop {
        let mut header = String::new();
        let read = reader
            .read_line(&mut header)
            .map_err(|error| format!("failed to read headers: {error}"))?;
        if read == 0 || header.trim().is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':')
            && name.trim().eq_ignore_ascii_case("content-length")
        {
            content_length = value.trim().parse().unwrap_or(0);
        }
    }

    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        reader
            .read_exact(&mut body)
            .map_err(|error| format!("failed to read body: {error}"))?;
    }
    let body = String::from_utf8(body).map_err(|_| "body is not valid UTF-8".to_owned())?;
    Ok((method, path, body))
}

fn handle_request(
    state: &Arc<Mutex<SharedState>>,
    mission: &str,
    method: &str,
    path: &str,
    body: &str,
) -> (u16, Value) {
    // Ignore any query string; none of these endpoints take one.
    let path = path.split('?').next().unwrap_or(path);
    match (method, path) {
        ("GET", "/v1/status") => {
            let state = state.lock().unwrap();
            (
                200,
                json!({
                    "mission": mission,
                    "frame": state.frame,
                    "overrides": state.overrides.channels(),
                }),
            )
        }
        ("GET", "/v1/control/input") => {
            let state = state.lock().unwrap();
            (200, json!({ "overrides": state.overrides.channels() }))
        }
        ("POST", "/v1/control/input") => {
            let value = match parse_body(body) {
                Ok(value) => value,
                Err(error) => return (400, json!({ "error": error })),
            };
            let patches = match shock2vr::input::remote::parse_input_patches(&value) {
                Ok(patches) => patches,
                Err(error) => return (400, json!({ "error": error })),
            };
            let mut state = state.lock().unwrap();
            for (channel, value) in patches {
                // Stop at the first bad channel so the caller gets an
                // actionable error instead of a silent partial apply.
                if let Err(error) = state.overrides.set(&channel, value) {
                    return (400, json!({ "error": error }));
                }
            }
            (
                200,
                json!({ "success": true, "overrides": state.overrides.channels() }),
            )
        }
        ("POST", "/v1/control/input/clear") => {
            let mut state = state.lock().unwrap();
            state.overrides.clear();
            (200, json!({ "success": true, "overrides": {} }))
        }
        ("POST", "/v1/input/action") => {
            let value = match parse_body(body) {
                Ok(value) => value,
                Err(error) => return (400, json!({ "error": error })),
            };
            let name = match value.get("action").and_then(Value::as_str) {
                Some(name) => name,
                None => {
                    return (400, json!({ "error": "expected {\"action\": \"<name>\"}" }));
                }
            };
            match name.parse::<InputAction>() {
                Ok(action) => {
                    state.lock().unwrap().pending_actions.push(action);
                    (200, json!({ "success": true, "action": name }))
                }
                Err(_) => (
                    400,
                    json!({
                        "error": format!("unknown action '{name}'"),
                        "actions": action_names(),
                    }),
                ),
            }
        }
        ("GET", "/v1/input/actions") => (200, json!({ "actions": action_names() })),
        _ => (
            400,
            json!({
                "error": format!("unknown endpoint {method} {path}"),
                "endpoints": [
                    "GET /v1/status",
                    "GET /v1/control/input",
                    "POST /v1/control/input",
                    "POST /v1/control/input/clear",
                    "POST /v1/input/action",
                    "GET /v1/input/actions",
                ],
            }),
        ),
    }
}

fn action_names() -> Vec<&'static str> {
    InputAction::all().iter().map(|a| a.as_str()).collect()
}

/// Parse a JSON body regardless of `Content-Type` (matching the debug runtime's
/// lenient behavior, so plain `curl -d '...'` works); an empty body is `{}`.
fn parse_body(body: &str) -> Result<Value, String> {
    if body.trim().is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_str(body).map_err(|error| format!("invalid JSON body: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_port() {
        assert_eq!(parse_port(" 8080\n"), Some(8080));
        assert_eq!(parse_port(""), None);
        assert_eq!(parse_port("0"), None);
        assert_eq!(parse_port("not-a-port"), None);
    }

    #[test]
    fn patches_and_clears_overrides() {
        let state = Arc::new(Mutex::new(SharedState::default()));
        let (status, _) = handle_request(
            &state,
            "medsci1.mis",
            "POST",
            "/v1/control/input",
            "{\"right_hand.trigger\": 1.0}",
        );
        assert_eq!(status, 200);

        let mut input = InputContext::default();
        state.lock().unwrap().overrides.apply(&mut input);
        assert_eq!(input.right_hand.trigger_value, 1.0);

        let (status, _) =
            handle_request(&state, "medsci1.mis", "POST", "/v1/control/input/clear", "");
        assert_eq!(status, 200);
        assert!(state.lock().unwrap().overrides.is_empty());
    }

    #[test]
    fn rejects_unknown_channels_and_actions() {
        let state = Arc::new(Mutex::new(SharedState::default()));
        let (status, _) = handle_request(
            &state,
            "medsci1.mis",
            "POST",
            "/v1/control/input",
            "{\"bogus\": 1.0}",
        );
        assert_eq!(status, 400);
        let (status, _) = handle_request(
            &state,
            "medsci1.mis",
            "POST",
            "/v1/input/action",
            "{\"action\": \"NotAnAction\"}",
        );
        assert_eq!(status, 400);
        assert!(state.lock().unwrap().pending_actions.is_empty());
    }
}
