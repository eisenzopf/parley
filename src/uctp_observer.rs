//! Bounded, leased journal observation. A slow observer never blocks commands.
use crate::{runtime::AppState, ApiError, Result};
use rvoip_uctp::{application::ApplicationContext, envelope::UctpEnvelope, types::MessageType};
use serde_json::{json, Value};
use std::{sync::Mutex, time::Duration};
use tokio::{sync::mpsc, task::JoinHandle};

const MAX_OBSERVERS: usize = 64;
const LEASE: Duration = Duration::from_secs(30);

#[derive(Default)]
pub(crate) struct Observers {
    entries: Mutex<Vec<Entry>>,
}
struct Entry {
    cid: String,
    outbound: mpsc::WeakSender<UctpEnvelope>,
    task: JoinHandle<()>,
}
impl Drop for Observers {
    fn drop(&mut self) {
        for entry in self.entries.get_mut().expect("observer lock").drain(..) {
            entry.task.abort();
        }
    }
}
impl Observers {
    pub fn subscribe(
        &self,
        state: AppState,
        context: ApplicationContext,
        tenant: String,
        cid: String,
        cursor: i64,
    ) -> Result<Value> {
        self.subscribe_for(state, context, tenant, cid, cursor, LEASE, MAX_OBSERVERS)
    }

    fn subscribe_for(
        &self,
        state: AppState,
        context: ApplicationContext,
        tenant: String,
        cid: String,
        mut cursor: i64,
        lease: Duration,
        cap: usize,
    ) -> Result<Value> {
        let mut entries = self.entries.lock().expect("observer lock");
        entries.retain(|entry| !entry.task.is_finished());
        if entries.iter().any(|entry| {
            entry.cid == cid
                && entry
                    .outbound
                    .upgrade()
                    .is_some_and(|out| out.same_channel(&context.outbound))
        }) {
            return Err(ApiError::conflict(
                "live subscription already active; use a snapshot or wait for lease expiry",
            ));
        }
        if entries.len() >= cap {
            return Err(ApiError::new(
                axum::http::StatusCode::TOO_MANY_REQUESTS,
                "observer-capacity",
                "Observer capacity reached",
                "live observer capacity reached; use cursor snapshots",
            ));
        }
        let id = format!("sub_{}", uuid::Uuid::new_v4().simple());
        let now = chrono::Utc::now();
        let requested_expiry = now + chrono::Duration::from_std(lease).expect("lease duration");
        let expiry = context
            .principal
            .expires_at
            .map(|auth| auth.min(requested_expiry))
            .unwrap_or(requested_expiry);
        let expiry_reason = if expiry < requested_expiry {
            "authorization_expired"
        } else {
            "lease_expired"
        };
        let effective_lease = (expiry - now).to_std().unwrap_or_default();
        let metadata = json!({"id":id,"expires_at":expiry.to_rfc3339(),"recovery":"conversation.subscribe with last observed cursor and a new request ID"});
        let deadline = tokio::time::Instant::now() + effective_lease;
        let outbound = context.outbound.downgrade();
        let entry_cid = cid.clone();
        let task = tokio::spawn(async move {
            let mut wake = state.live_events.subscribe();
            let mut tick = tokio::time::interval(Duration::from_millis(250));
            let reason = 'observe: loop {
                tokio::select! {
                    biased;
                    _=context.closed.cancelled()=>break "peer_closed",
                    _=tokio::time::sleep_until(deadline)=>break expiry_reason,
                    _=tick.tick()=>{},
                    _=wake.recv()=>{},
                }
                if context.principal.is_expired() {
                    break "authorization_expired";
                }
                let member =
                    state
                        .store
                        .conference_members(&tenant, &cid)
                        .ok()
                        .and_then(|members| {
                            members
                                .into_iter()
                                .find(|m| m.subject == context.principal.subject)
                        });
                let Some(member) = member else {
                    break "membership_unavailable";
                };
                let Ok(events) = state
                    .store
                    .conference_events(&tenant, &cid, &member, cursor, 100)
                else {
                    break "journal_unavailable";
                };
                for event in events {
                    let seq = event.seq;
                    let envelope=UctpEnvelope::new(MessageType::Unknown("conversation.event".into()),json!({"profile":super::uctp_commands::PROFILE,"subscription_id":id,"event":event})).with_cid(cid.clone());
                    // Only this bounded observer task waits for output. The
                    // journal remains authoritative if its lease expires here.
                    tokio::select! {
                        biased;
                        _=context.closed.cancelled()=>break 'observe "peer_closed",
                        _=tokio::time::sleep_until(deadline)=>break 'observe expiry_reason,
                        result=context.outbound.send(envelope)=>if result.is_err(){break 'observe "peer_closed";},
                    }
                    cursor = seq;
                }
            };
            // Best effort when capacity permits. Clients MUST honor the lease
            // even when an overloaded transport cannot deliver this notice.
            let _=context.outbound.try_send(UctpEnvelope::new(MessageType::Unknown("conversation.subscription_ended".into()),json!({"profile":super::uctp_commands::PROFILE,"subscription_id":id,"reason":reason,"cursor":cursor})).with_cid(cid));
        });
        entries.push(Entry {
            cid: entry_cid,
            outbound,
            task,
        });
        Ok(metadata)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        store::{conference::MemberInput, Store},
        App, Config,
    };
    use rvoip_auth_core::{AuthenticatedPrincipal, AuthenticationMethod};
    use rvoip_core::{IdentityAssurance, IdentityId};

    fn context(subject: &str, outbound: mpsc::Sender<UctpEnvelope>) -> ApplicationContext {
        ApplicationContext {
            principal: AuthenticatedPrincipal {
                subject: subject.into(),
                tenant: Some("ten_local".into()),
                scopes: vec![],
                issuer: None,
                expires_at: None,
                method: AuthenticationMethod::Bearer,
                assurance: IdentityAssurance::UserAuthorized {
                    identity: IdentityId::new(),
                    user_id: IdentityId::new(),
                    scopes: vec![],
                },
            },
            outbound,
            closed: Default::default(),
        }
    }

    #[tokio::test]
    async fn full_observer_expires_without_blocking_journal_or_leaking_tasks() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        cfg.sqlite_path = dir.path().join("observer.sqlite").display().to_string();
        cfg.blob_dir = dir.path().join("blobs").display().to_string();
        cfg.vapi_chat_mode = "fake".into();
        let app = App::new(cfg.clone(), Store::open(&cfg).unwrap()).unwrap();
        let members = app
            .state
            .store
            .create_conference(
                "ten_local",
                "conv_observer",
                &[MemberInput {
                    alias: "owner".into(),
                    name: "Owner".into(),
                    role: "owner".into(),
                    sms: None,
                    sip: None,
                }],
                "create_observer",
            )
            .unwrap();
        let observers = Observers::default();
        let (tx, mut rx) = mpsc::channel(1);
        tx.try_send(UctpEnvelope::new(
            MessageType::Ack,
            json!({"sentinel":true}),
        ))
        .unwrap();
        let lease = Duration::from_millis(75);
        let first = observers
            .subscribe_for(
                app.state.clone(),
                context(&members[0].subject, tx.clone()),
                "ten_local".into(),
                "conv_observer".into(),
                0,
                lease,
                1,
            )
            .unwrap();
        assert!(first["expires_at"].is_string());
        assert_eq!(
            observers
                .subscribe_for(
                    app.state.clone(),
                    context(&members[0].subject, tx.clone()),
                    "ten_local".into(),
                    "conv_observer".into(),
                    0,
                    lease,
                    1
                )
                .unwrap_err()
                .status,
            axum::http::StatusCode::CONFLICT
        );
        let (other, _) = mpsc::channel(1);
        assert_eq!(
            observers
                .subscribe_for(
                    app.state.clone(),
                    context(&members[0].subject, other),
                    "ten_local".into(),
                    "conv_observer".into(),
                    0,
                    lease,
                    1
                )
                .unwrap_err()
                .status,
            axum::http::StatusCode::TOO_MANY_REQUESTS
        );
        // Write task facts while the observer's output is deliberately full.
        for mid in ["msg_observer_a", "msg_observer_b"] {
            app.state
                .store
                .enqueue_conference_message(
                    "ten_local",
                    "conv_observer",
                    &members[0],
                    &members,
                    mid,
                    "Journal survives observer backpressure",
                    "text/plain",
                    "chat",
                    None,
                    mid,
                    "",
                )
                .unwrap();
        }
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if observers.entries.lock().unwrap()[0].task.is_finished() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(rx.recv().await.unwrap().payload["sentinel"], true);
        assert!(rx.try_recv().is_err());
        let facts = app
            .state
            .store
            .conference_events("ten_local", "conv_observer", &members[0], 0, 500)
            .unwrap();
        assert_eq!(
            facts
                .iter()
                .filter(|e| e.event_type == "message.accepted")
                .count(),
            2
        );
        // Expired registry entries are reclaimed, and the last observed cursor
        // can recover every event through a new lease.
        let ctx = context(&members[0].subject, tx);
        let closed = ctx.closed.clone();
        let next = observers
            .subscribe_for(
                app.state.clone(),
                ctx,
                "ten_local".into(),
                "conv_observer".into(),
                0,
                Duration::from_secs(1),
                1,
            )
            .unwrap();
        assert_ne!(next["id"], first["id"]);
        for fact in facts {
            let event = tokio::time::timeout(Duration::from_secs(1), rx.recv())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(event.payload["event"]["seq"], fact.seq);
        }
        closed.cancel();
        let ended = tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(ended.payload["reason"], "peer_closed");
        let last = app
            .state
            .store
            .conference_events("ten_local", "conv_observer", &members[0], 0, 500)
            .unwrap()
            .last()
            .unwrap()
            .seq;
        let (auth_tx, mut auth_rx) = mpsc::channel(1);
        let mut auth = context(&members[0].subject, auth_tx);
        auth.principal.expires_at = Some(chrono::Utc::now() + chrono::Duration::milliseconds(35));
        observers
            .subscribe_for(
                app.state.clone(),
                auth,
                "ten_local".into(),
                "conv_observer".into(),
                last,
                Duration::from_secs(1),
                1,
            )
            .unwrap();
        let ended = tokio::time::timeout(Duration::from_secs(1), auth_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(ended.payload["reason"], "authorization_expired");
    }
}
