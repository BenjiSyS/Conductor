//! Pending permission requests. The agent waits on a request; the UI (or a
//! remote client) resolves it. Full Access skips the wait except for
//! dangerous actions, which always ask.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, oneshot};

use crate::events::EngineEvent;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub id: String,
    pub goal_id: Option<String>,
    pub capability: String,
    pub detail: String,
    pub risk: String,
}

#[async_trait]
pub trait Approver: Send + Sync {
    async fn approve(&self, req: ApprovalRequest) -> bool;
}

/// Approver backed by UI events.
pub struct UiApprovals {
    pending: Mutex<HashMap<String, (ApprovalRequest, oneshot::Sender<bool>)>>,
    events: broadcast::Sender<EngineEvent>,
}

/// A cancelled approval future must also remove its UI request. Resolution
/// consumes the entry first, so normal completion cannot send a second event.
struct PendingApproval<'a> {
    owner: &'a UiApprovals,
    id: String,
}
impl Drop for PendingApproval<'_> {
    fn drop(&mut self) {
        self.owner.resolve(&self.id, false);
    }
}

impl UiApprovals {
    pub fn new(events: broadcast::Sender<EngineEvent>) -> Arc<Self> {
        Arc::new(Self {
            pending: Mutex::new(HashMap::new()),
            events,
        })
    }

    pub fn resolve(&self, id: &str, allowed: bool) -> bool {
        let entry = self.pending.lock().expect("approvals lock").remove(id);
        match entry {
            Some((_, tx)) => {
                let _ = tx.send(allowed);
                let _ = self.events.send(EngineEvent::ApprovalResolved {
                    id: id.into(),
                    allowed,
                });
                true
            }
            None => false,
        }
    }

    pub fn pending(&self) -> Vec<ApprovalRequest> {
        self.pending
            .lock()
            .expect("approvals lock")
            .values()
            .map(|(r, _)| r.clone())
            .collect()
    }

    /// Deny everything outstanding (Stop / Emergency Stop).
    pub fn deny_all(&self) {
        let drained: Vec<_> = self
            .pending
            .lock()
            .expect("approvals lock")
            .drain()
            .collect();
        for (id, (_, tx)) in drained {
            let _ = tx.send(false);
            let _ = self
                .events
                .send(EngineEvent::ApprovalResolved { id, allowed: false });
        }
    }
}

#[async_trait]
impl Approver for UiApprovals {
    async fn approve(&self, req: ApprovalRequest) -> bool {
        let (tx, rx) = oneshot::channel();
        let pending = PendingApproval {
            owner: self,
            id: req.id.clone(),
        };
        self.pending
            .lock()
            .expect("approvals lock")
            .insert(req.id.clone(), (req.clone(), tx));
        let _ = self.events.send(EngineEvent::Approval {
            id: req.id.clone(),
            goal_id: req.goal_id.clone(),
            capability: req.capability.clone(),
            detail: req.detail.clone(),
            risk: req.risk.clone(),
        });
        let allowed = rx.await.unwrap_or(false);
        drop(pending);
        allowed
    }
}

/// Non-interactive approver: allow or deny everything that reaches it.
pub struct Fixed(pub bool);

#[async_trait]
impl Approver for Fixed {
    async fn approve(&self, _req: ApprovalRequest) -> bool {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn dropped_waiter_removes_request_and_notifies_ui_once() {
        let (events, mut receiver) = broadcast::channel(8);
        let approvals = UiApprovals::new(events);
        let owner = approvals.clone();
        let waiter = tokio::spawn(async move {
            owner
                .approve(ApprovalRequest {
                    id: "cancelled".into(),
                    goal_id: None,
                    capability: "file.write".into(),
                    detail: "fixture".into(),
                    risk: "normal".into(),
                })
                .await
        });
        assert!(matches!(
            receiver.recv().await.unwrap(),
            EngineEvent::Approval { .. }
        ));
        assert_eq!(
            approvals.pending().len(),
            1,
            "Request must exist before its UI event"
        );
        waiter.abort();
        assert!(waiter.await.unwrap_err().is_cancelled());
        assert!(approvals.pending().is_empty());
        assert!(matches!(
            receiver.recv().await.unwrap(),
            EngineEvent::ApprovalResolved { allowed: false, .. }
        ));
        assert!(!approvals.resolve("cancelled", true));
        assert!(receiver.try_recv().is_err());
    }
}
