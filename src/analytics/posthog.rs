use posthog_rs::{Client, Event};
use serde_json::Value;

const POSTHOG_API_KEY: &str = "phc_aNlTw2xAUQKd9zTovXeYheEUpQpEhplehCK5r1e31HR";

pub async fn build_client() -> Option<Client> {
    Some(posthog_rs::client(POSTHOG_API_KEY).await)
}

fn add_common_props(event: &mut Event) {
    let _ = event.insert_prop("tempo_app_id", "presto");
    let _ = event.insert_prop("$lib", "presto");
    let _ = event.insert_prop("$lib_version", env!("CARGO_PKG_VERSION"));
    let _ = event.insert_prop("os", std::env::consts::OS);
    let _ = event.insert_prop("arch", std::env::consts::ARCH);
}

pub async fn capture(client: &Client, distinct_id: &str, event_name: &str, properties: Value) {
    let mut event = Event::new(event_name, distinct_id);
    add_common_props(&mut event);

    if let Value::Object(map) = properties {
        for (k, v) in map {
            let _ = event.insert_prop(k, v);
        }
    }

    let _ = client.capture(event).await;
}

pub async fn alias(client: &Client, previous_id: &str, new_id: &str) {
    let mut event = Event::new("$create_alias", new_id);
    let _ = event.insert_prop("alias", previous_id);
    let _ = event.insert_prop("tempo_app_id", "presto");

    let _ = client.capture(event).await;
}

pub async fn identify(client: &Client, distinct_id: &str, properties: Value) {
    let mut event = Event::new("$identify", distinct_id);
    let _ = event.insert_prop("tempo_app_id", "presto");
    let _ = event.insert_prop("$set", properties);

    let _ = client.capture(event).await;
}
