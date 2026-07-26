//! Off-thread AI path queries.
//!
//! A* (and its unreachable-goal fallback chain) is the most expensive
//! per-frame AI work — up to a few milliseconds per query on desktop and a
//! multiple of that on Quest. Instead of computing routes inside the game
//! update, steering SUBMITS a query and keeps following its stale path; a
//! dedicated worker thread computes the route in parallel with the frame
//! (simulation + rendering) and the strategy ADOPTS the result on a later
//! frame. Route adoption timing therefore jitters by a frame or two — fine
//! for AI, and the frame never blocks on a search.
//!
//! One request per entity is in flight at a time (a newer submit while one
//! is pending is dropped — the eventual result reflects a goal at most a
//! submit-cooldown old, and the strategy discards results that drifted too
//! far). The worker exits when the owning `AsyncPathfinding` is dropped
//! (mission unload drops the world, which drops the unique).

use cgmath::Vector3;
use dark::mission::path_database::MovementBits;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, mpsc};

use super::{AiPathOutcome, AiPathRecord, PathfindingService};

/// A path query for one AI, computed on the worker thread
pub struct PathQueryRequest {
    /// EntityId::inner() of the requesting AI
    pub entity: u64,
    pub start: Vector3<f32>,
    pub goal: Vector3<f32>,
    pub movement_bits: MovementBits,
    /// Mission time (seconds) at submit, for expiring the entity's
    /// steering-reported blocked crossings (see
    /// `PathfindingService::report_blocked_link`)
    pub now_seconds: f32,
}

/// The computed route (or lack of one) for an entity's latest request
#[derive(Clone)]
pub struct PathQueryResponse {
    /// The goal the route was computed against (echoed from the request, so
    /// the consumer can discard results whose goal has since drifted)
    pub goal: Vector3<f32>,
    pub outcome: AiPathOutcome,
    /// Route waypoints; empty when `outcome` is `Failed`
    pub waypoints: Vec<Vector3<f32>>,
}

struct SharedState {
    /// Completed responses, newest per entity, until taken
    results: Mutex<HashMap<u64, PathQueryResponse>>,
    /// Entities with a request in flight (submitted, not yet completed)
    pending: Mutex<HashSet<u64>>,
}

/// Handle to the pathfinding worker thread: submit queries, poll results
pub struct AsyncPathfinding {
    tx: mpsc::Sender<PathQueryRequest>,
    shared: Arc<SharedState>,
}

impl AsyncPathfinding {
    /// Spawn the worker thread against a service. The thread exits when
    /// this handle is dropped.
    pub fn spawn(service: Arc<PathfindingService>) -> Self {
        let (tx, rx) = mpsc::channel::<PathQueryRequest>();
        let shared = Arc::new(SharedState {
            results: Mutex::new(HashMap::new()),
            pending: Mutex::new(HashSet::new()),
        });
        let worker_shared = shared.clone();
        std::thread::Builder::new()
            .name("ai-pathfinding".to_string())
            .spawn(move || {
                while let Ok(request) = rx.recv() {
                    let mut outcome = AiPathOutcome::Full;
                    // Crossings ANY AI's steering reported as physically
                    // blocked (stall mid-route) are excluded, so re-paths -
                    // including fresh arrivals' - route around the obstacle
                    let avoid = service.blocked_links(request.now_seconds);
                    let path = service
                        .find_path_avoiding(
                            request.start,
                            request.goal,
                            request.movement_bits,
                            &avoid,
                        )
                        .or_else(|| {
                            outcome = AiPathOutcome::Partial;
                            service.find_path_toward_avoiding(
                                request.start,
                                request.goal,
                                request.movement_bits,
                                &avoid,
                            )
                        });
                    let waypoints = match path {
                        Some(waypoints) => waypoints,
                        None => {
                            outcome = AiPathOutcome::Failed;
                            Vec::new()
                        }
                    };
                    service.record_ai_path(
                        request.entity,
                        AiPathRecord {
                            goal: request.goal,
                            waypoints: waypoints.clone(),
                            outcome,
                        },
                    );
                    if let Ok(mut results) = worker_shared.results.lock() {
                        results.insert(
                            request.entity,
                            PathQueryResponse {
                                goal: request.goal,
                                outcome,
                                waypoints,
                            },
                        );
                    }
                    if let Ok(mut pending) = worker_shared.pending.lock() {
                        pending.remove(&request.entity);
                    }
                }
            })
            .expect("failed to spawn ai-pathfinding worker thread");
        Self { tx, shared }
    }

    /// Queue a query unless one is already in flight for this entity.
    /// Returns whether the request was accepted.
    pub fn submit(&self, request: PathQueryRequest) -> bool {
        let entity = request.entity;
        {
            let Ok(mut pending) = self.shared.pending.lock() else {
                return false;
            };
            if !pending.insert(entity) {
                return false;
            }
        }
        if self.tx.send(request).is_err() {
            // Worker died (should not happen); un-mark so callers can retry
            if let Ok(mut pending) = self.shared.pending.lock() {
                pending.remove(&entity);
            }
            return false;
        }
        true
    }

    /// Whether a request for this entity is still being computed
    pub fn is_pending(&self, entity: u64) -> bool {
        self.shared
            .pending
            .lock()
            .map(|pending| pending.contains(&entity))
            .unwrap_or(false)
    }

    /// Take the completed response for this entity, if any
    pub fn take_result(&self, entity: u64) -> Option<PathQueryResponse> {
        self.shared
            .results
            .lock()
            .ok()
            .and_then(|mut results| results.remove(&entity))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn wait_for_result(async_pf: &AsyncPathfinding, entity: u64) -> Option<PathQueryResponse> {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if let Some(response) = async_pf.take_result(entity) {
                return Some(response);
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        None
    }

    #[test]
    fn worker_computes_routes_off_thread() {
        let service = Arc::new(PathfindingService::new(Arc::new(
            crate::pathfinding::tests::three_cell_db(
                dark::mission::path_database::PathCellFlags::empty(),
            ),
        )));
        let async_pf = AsyncPathfinding::spawn(service);

        assert!(async_pf.submit(PathQueryRequest {
            entity: 7,
            start: cgmath::vec3(1.0, 0.0, 1.0),
            goal: cgmath::vec3(5.0, 0.0, 1.0),
            movement_bits: MovementBits::WALK,
            now_seconds: 0.0,
        }));
        let response = wait_for_result(&async_pf, 7).expect("worker must respond");
        assert_eq!(response.outcome, AiPathOutcome::Full);
        assert!(!response.waypoints.is_empty());
        assert_eq!(response.goal, cgmath::vec3(5.0, 0.0, 1.0));
        // Result was taken; nothing pending or left over
        assert!(!async_pf.is_pending(7));
        assert!(async_pf.take_result(7).is_none());
    }

    #[test]
    fn unreachable_goal_reports_failed_or_partial() {
        // Goal far outside every cell: find_path fails; find_path_toward
        // from the start cell has no closer reachable cell either
        let service = Arc::new(PathfindingService::new(Arc::new(
            crate::pathfinding::tests::three_cell_db(
                dark::mission::path_database::PathCellFlags::empty(),
            ),
        )));
        let async_pf = AsyncPathfinding::spawn(service.clone());
        assert!(async_pf.submit(PathQueryRequest {
            entity: 9,
            start: cgmath::vec3(5.0, 0.0, 1.0),
            goal: cgmath::vec3(500.0, 0.0, 500.0),
            movement_bits: MovementBits::WALK,
            now_seconds: 0.0,
        }));
        let response = wait_for_result(&async_pf, 9).expect("worker must respond");
        assert_ne!(response.outcome, AiPathOutcome::Full);
        // The attempt is introspectable regardless of outcome
        assert!(
            service.ai_paths().iter().any(|(entity, _)| *entity == 9),
            "worker must record the attempt for GET /v1/ai/paths"
        );
    }

    #[test]
    fn worker_applies_blocked_links() {
        // Cell 0 -> goal in cell 2, but an AI reported the 1 -> 2 crossing
        // blocked: the worker must return a PARTIAL route ending at cell 1
        // instead of the full route through the obstacle - for EVERY AI,
        // not just the reporter.
        let service = Arc::new(PathfindingService::new(Arc::new(
            crate::pathfinding::tests::three_cell_db(
                dark::mission::path_database::PathCellFlags::empty(),
            ),
        )));
        service.report_blocked_link(1, 2, 0.0);
        let async_pf = AsyncPathfinding::spawn(service.clone());
        assert!(async_pf.submit(PathQueryRequest {
            entity: 11,
            start: cgmath::vec3(1.0, 0.0, 1.0),
            goal: cgmath::vec3(5.0, 0.0, 1.0),
            movement_bits: MovementBits::WALK,
            now_seconds: 1.0,
        }));
        let response = wait_for_result(&async_pf, 11).expect("worker must respond");
        assert_eq!(response.outcome, AiPathOutcome::Partial);
        assert_eq!(
            *response.waypoints.last().unwrap(),
            cgmath::vec3(3.0, 0.0, 1.0),
            "partial route must end at cell 1's center, before the blockage"
        );

        // The exclusion is shared: another entity's query avoids it too
        assert!(async_pf.submit(PathQueryRequest {
            entity: 12,
            start: cgmath::vec3(1.0, 0.0, 1.0),
            goal: cgmath::vec3(5.0, 0.0, 1.0),
            movement_bits: MovementBits::WALK,
            now_seconds: 1.0,
        }));
        let response = wait_for_result(&async_pf, 12).expect("worker must respond");
        assert_eq!(response.outcome, AiPathOutcome::Partial);
    }
}
