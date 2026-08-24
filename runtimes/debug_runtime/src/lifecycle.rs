//! Process-lifetime concerns for the debug runtime: how it reports the port it
//! bound, and when it gives up and exits on its own.
//!
//! Both exist because runtimes are launched by agents and CI jobs that can die
//! without cleaning up. A leaked runtime holds ~700 MB and a port for as long
//! as the machine stays up, and a caller that has to *assume* a port ends up
//! talking to somebody else's runtime.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Default idle timeout before the runtime exits by itself.
///
/// Chosen to sit between two failure modes:
/// - Too short is destructive: an agent legitimately parks a paused runtime
///   while it reasons, reads screenshots, or waits on another build - gaps of
///   several minutes are normal, and killing a live session's runtime is far
///   worse than leaking one.
/// - Too long is pointless: the leak this guards against is an *orphan* (its
///   owner crashed or was killed), and an orphan must not survive a working
///   day - by then several have piled up, each holding ~700 MB.
///
/// 30 minutes is comfortably above any plausible think-time gap and still
/// bounds a leak to half an hour. It is on by DEFAULT deliberately: the
/// callers most likely to orphan a runtime (a `curl`-driven agent, a hand
/// launch) are exactly the ones who would never pass an opt-in flag.
///
/// Note the runtime is paused by default and only advances on `/v1/step`, so
/// "no HTTP request at all" is a sound liveness signal; "not stepping" would
/// NOT be. The one real false positive follows from that: a human running
/// `--visible`, free-running a scene and watching it for half an hour without
/// touching HTTP, gets shut down. `--idle-timeout-secs 0` is the escape hatch
/// (it is named in the flag's help text for exactly that moment).
pub const DEFAULT_IDLE_TIMEOUT_SECS: u64 = 30 * 60;

/// How often the watchdog task re-checks. Small relative to any sane timeout,
/// so the reported idle time is accurate to about a second.
pub const IDLE_POLL_INTERVAL: Duration = Duration::from_secs(1);

/// The line printed once the HTTP server is bound - before it is usable - so
/// any caller (shell, agent, CI) can read the real port instead of assuming
/// it.
///
/// It carries the identity of the process too, because "where do I connect"
/// is only half the discovery question; the other half is "what is running,
/// and is it mine?", which today is answered with `lsof` and guesswork.
/// `instance_id` is empty when the launcher did not pass one (a hand launch
/// rather than the SDK) - the field is always present so a parser can rely on
/// it.
pub fn port_marker_line(addr: SocketAddr, pid: u32, instance_id: Option<&str>) -> String {
    // --instance-id is opaque caller-supplied text; keep it to characters that
    // cannot break the `key=value` framing (or forge a second marker line).
    let instance_id: String = instance_id
        .unwrap_or("")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '?'
            }
        })
        .collect();
    format!(
        "SHOCK2QUEST_PORT port={} pid={} instance_id={} address={}",
        addr.port(),
        pid,
        instance_id,
        addr
    )
}

/// The line printed when the watchdog decides to exit.
pub fn idle_exit_marker_line(idle: Duration, timeout: Duration) -> String {
    format!(
        "SHOCK2QUEST_IDLE_EXIT idle_secs={:.1} timeout_secs={}",
        idle.as_secs_f64(),
        timeout.as_secs()
    )
}

#[derive(Debug)]
struct IdleState {
    /// When the most recent request started or finished.
    last_activity: Instant,
    /// Requests currently being served. A long-running request (a big
    /// `/v1/step`) must never look like idleness.
    in_flight: u32,
}

/// Tracks HTTP activity so the runtime can exit after a quiet period.
///
/// The clock is injected (`now` is a parameter) so the decision logic is
/// unit-testable without sleeping.
#[derive(Debug)]
pub struct IdleWatchdog {
    /// `None` disables the watchdog entirely (`--idle-timeout-secs 0`).
    timeout: Option<Duration>,
    state: Mutex<IdleState>,
}

impl IdleWatchdog {
    pub fn new(timeout: Option<Duration>, now: Instant) -> Self {
        Self {
            timeout,
            state: Mutex::new(IdleState {
                last_activity: now,
                in_flight: 0,
            }),
        }
    }

    pub fn timeout(&self) -> Option<Duration> {
        self.timeout
    }

    /// Mark a request as in flight until the returned guard is dropped.
    ///
    /// The guard exists because a client can vanish mid-request (curl Ctrl-C,
    /// an aborted fetch, a killed session - exactly the orphan case this
    /// watchdog is for), in which case the server future is dropped rather
    /// than run to completion. A hand-written "decrement afterwards" would
    /// then never run and leave the runtime permanently "busy", silently
    /// disabling the watchdog forever.
    pub fn request_scope(self: &Arc<Self>, now: Instant) -> ActivityGuard {
        self.request_started(now);
        ActivityGuard {
            watchdog: Arc::clone(self),
        }
    }

    fn request_started(&self, now: Instant) {
        let mut state = self.state.lock().unwrap();
        state.last_activity = now;
        state.in_flight += 1;
    }

    fn request_finished(&self, now: Instant) {
        let mut state = self.state.lock().unwrap();
        state.last_activity = now;
        state.in_flight = state.in_flight.saturating_sub(1);
    }

    /// How long the runtime has been idle, or `None` while a request is in
    /// flight (a runtime serving a request is by definition not idle).
    fn idle_for(&self, now: Instant) -> Option<Duration> {
        let state = self.state.lock().unwrap();
        if state.in_flight > 0 {
            return None;
        }
        Some(now.saturating_duration_since(state.last_activity))
    }

    /// The elapsed idle time when the runtime should exit, else `None`.
    pub fn expired(&self, now: Instant) -> Option<Duration> {
        let timeout = self.timeout?;
        let idle = self.idle_for(now)?;
        (idle >= timeout).then_some(idle)
    }
}

/// Keeps the runtime marked busy for as long as one request lives. Dropping
/// it - normally, or because the request was cancelled - ends that request's
/// claim and restarts the idle clock.
pub struct ActivityGuard {
    watchdog: Arc<IdleWatchdog>,
}

impl Drop for ActivityGuard {
    fn drop(&mut self) {
        self.watchdog.request_finished(Instant::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    fn watchdog(secs: u64) -> (IdleWatchdog, Instant) {
        let start = Instant::now();
        (
            IdleWatchdog::new(Some(Duration::from_secs(secs)), start),
            start,
        )
    }

    #[test]
    fn watchdog_fires_after_the_timeout_elapses() {
        let (watchdog, start) = watchdog(60);

        assert_eq!(watchdog.expired(start + Duration::from_secs(59)), None);
        assert_eq!(
            watchdog.expired(start + Duration::from_secs(60)),
            Some(Duration::from_secs(60))
        );
    }

    #[test]
    fn any_request_resets_the_idle_clock() {
        let (watchdog, start) = watchdog(60);

        let request_at = start + Duration::from_secs(59);
        watchdog.request_started(request_at);
        watchdog.request_finished(request_at);

        // 61s after start, but only 2s after the request.
        assert_eq!(watchdog.expired(start + Duration::from_secs(61)), None);
        assert_eq!(
            watchdog.expired(request_at + Duration::from_secs(60)),
            Some(Duration::from_secs(60))
        );
    }

    #[test]
    fn an_in_flight_request_is_never_idle() {
        let (watchdog, start) = watchdog(60);

        // A request that outlives the timeout (e.g. a long /v1/step) must not
        // let the watchdog kill the runtime out from under its caller.
        watchdog.request_started(start);
        let much_later = start + Duration::from_secs(600);
        assert_eq!(watchdog.idle_for(much_later), None);
        assert_eq!(watchdog.expired(much_later), None);

        watchdog.request_finished(much_later);
        assert_eq!(watchdog.expired(much_later), None);
        assert_eq!(
            watchdog.expired(much_later + Duration::from_secs(60)),
            Some(Duration::from_secs(60))
        );
    }

    #[test]
    fn overlapping_requests_keep_the_runtime_busy_until_the_last_one_ends() {
        let (watchdog, start) = watchdog(60);

        watchdog.request_started(start);
        watchdog.request_started(start);
        watchdog.request_finished(start);
        assert_eq!(watchdog.idle_for(start + Duration::from_secs(600)), None);

        watchdog.request_finished(start);
        assert_eq!(
            watchdog.idle_for(start + Duration::from_secs(600)),
            Some(Duration::from_secs(600))
        );
    }

    #[test]
    fn dropping_a_request_scope_releases_the_runtime() {
        // A cancelled request (client hung up mid-request) unwinds by dropping
        // the response future, so the release must ride on Drop - otherwise
        // one aborted request would wedge the runtime "busy" forever and
        // silently disable the watchdog.
        let start = Instant::now();
        let watchdog = Arc::new(IdleWatchdog::new(Some(Duration::from_secs(60)), start));

        let guard = watchdog.request_scope(start);
        assert_eq!(watchdog.idle_for(start + Duration::from_secs(600)), None);

        drop(guard);
        assert!(watchdog.idle_for(Instant::now()).is_some());
    }

    #[test]
    fn a_zero_timeout_disables_the_watchdog() {
        let start = Instant::now();
        let watchdog = IdleWatchdog::new(None, start);

        assert_eq!(watchdog.expired(start + Duration::from_secs(86_400)), None);
    }

    #[test]
    fn port_marker_reports_the_resolved_port_not_the_request() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 54321);

        assert_eq!(
            port_marker_line(addr, 4242, Some("abc-123")),
            "SHOCK2QUEST_PORT port=54321 pid=4242 instance_id=abc-123 address=127.0.0.1:54321"
        );
    }

    #[test]
    fn port_marker_neutralizes_an_instance_id_that_would_break_parsing() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8080);

        assert_eq!(
            port_marker_line(addr, 7, Some("a b\nSHOCK2QUEST_PORT port=1")),
            "SHOCK2QUEST_PORT port=8080 pid=7 instance_id=a?b?SHOCK2QUEST_PORT?port?1 address=127.0.0.1:8080"
        );
    }

    #[test]
    fn port_marker_keeps_the_instance_id_field_when_there_is_none() {
        // A hand launch passes no --instance-id; the field stays present (and
        // empty) so a parser never has to special-case a missing key.
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8080);

        assert_eq!(
            port_marker_line(addr, 7, None),
            "SHOCK2QUEST_PORT port=8080 pid=7 instance_id= address=127.0.0.1:8080"
        );
    }

    #[test]
    fn idle_exit_marker_reports_both_durations() {
        assert_eq!(
            idle_exit_marker_line(Duration::from_millis(1_800_400), Duration::from_secs(1_800)),
            "SHOCK2QUEST_IDLE_EXIT idle_secs=1800.4 timeout_secs=1800"
        );
    }
}
