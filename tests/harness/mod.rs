pub mod cleanup;
pub mod client;
pub mod context_server;
pub mod fixtures;
pub mod mock_server;
pub mod mqtt_receiver;
pub mod notification_receiver;
pub mod runtime;
pub mod trial;

use client::NgsiLdClient;
use std::sync::{Arc, Mutex};

pub const BASE_URL: &str = "http://127.0.0.1:1056";
pub const DATABASE_URL: &str = "postgres://postgres:postgres@127.0.0.1:5432/postgres";

#[derive(Default)]
struct TrackedResources {
    entities: Vec<String>,
    temporal_entities: Vec<String>,
    contexts: Vec<String>,
    registrations: Vec<String>,
    csource_subscriptions: Vec<String>,
    subscriptions: Vec<String>,
    entity_maps: Vec<String>,
    snapshots: Vec<String>,
}

#[derive(Clone)]
pub struct TestCtx {
    pub client: NgsiLdClient,
    tracked: Arc<Mutex<TrackedResources>>,
}

impl TestCtx {
    pub fn new() -> Self {
        Self {
            client: NgsiLdClient::new(BASE_URL),
            tracked: Arc::new(Mutex::new(TrackedResources::default())),
        }
    }

    pub fn entity_id(&self, prefix: &str) -> String {
        let id = format!("{prefix}:{}", uuid::Uuid::new_v4());
        self.track_entity(&id);
        id
    }

    pub fn track_entity(&self, id: &str) {
        self.tracked.lock().unwrap().entities.push(id.to_string());
    }

    pub fn track_temporal_entity(&self, id: &str) {
        self.tracked.lock().unwrap().temporal_entities.push(id.to_string());
    }

    pub fn track_context(&self, id: &str) {
        self.tracked.lock().unwrap().contexts.push(id.to_string());
    }

    pub fn track_registration(&self, id: &str) {
        self.tracked.lock().unwrap().registrations.push(id.to_string());
    }

    pub fn track_csource_subscription(&self, id: &str) {
        self.tracked.lock().unwrap().csource_subscriptions.push(id.to_string());
    }

    pub fn track_subscription(&self, id: &str) {
        self.tracked.lock().unwrap().subscriptions.push(id.to_string());
    }

    pub fn track_entity_map(&self, id: &str) {
        self.tracked.lock().unwrap().entity_maps.push(id.to_string());
    }

    pub fn track_snapshot(&self, id: &str) {
        self.tracked.lock().unwrap().snapshots.push(id.to_string());
    }

    pub async fn cleanup(&self) {
        let resources = {
            let mut t = self.tracked.lock().unwrap();
            std::mem::take(&mut *t)
        };

        for id in &resources.snapshots {
            let _ = self.client.delete_snapshot(id).await;
        }
        for id in &resources.entity_maps {
            let _ = self.client.delete_entity_map(id).await;
        }
        for id in &resources.subscriptions {
            let _ = self.client.delete_subscription(id).await;
        }
        for id in &resources.csource_subscriptions {
            let _ = self.client.delete_csource_subscription(id).await;
        }
        for id in &resources.registrations {
            let _ = self.client.delete_registration(id).await;
        }
        for id in &resources.temporal_entities {
            let _ = self.client.delete_temporal_entity(id).await;
        }
        for id in &resources.entities {
            let _ = self.client.delete_entity(id).await;
        }
        for id in &resources.contexts {
            let _ = self.client.delete_context(id).await;
        }

        cleanup::clean_database(DATABASE_URL).await;
    }
}
