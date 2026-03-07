//! Snapshot-like structure tests for JSON and TOON outputs.

use std::process::Output;

use axum::routing::get;
use axum::{Json, Router};
use serde_json::Value;

mod common;
use common::{seed_local_session, test_command, TestConfigBuilder};

fn run_structured(temp: &tempfile::TempDir, flag: &str, args: &[&str]) -> (Output, Value) {
    let mut cmd = test_command(temp);
    let all_args: Vec<&str> = std::iter::once(flag).chain(args.iter().copied()).collect();
    let output = cmd.args(all_args).output().expect("command should run");
    assert!(output.status.success(), "command failed: {output:?}");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed = if flag == "-j" {
        serde_json::from_str(stdout.trim()).expect("valid JSON output")
    } else {
        toon_format::decode_default(stdout.trim()).expect("valid TOON output")
    };

    (output, parsed)
}

fn run_both(temp: &tempfile::TempDir, args: &[&str]) -> (Output, Value, Output, Value) {
    let (json_out, json_val) = run_structured(temp, "-j", args);
    let (toon_out, toon_val) = run_structured(temp, "-t", args);
    (json_out, json_val, toon_out, toon_val)
}

fn assert_clean_stderr(output: &Output) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.trim().is_empty(),
        "structured mode should not write to stderr: {stderr}"
    );
}

fn assert_json_toon_equivalent(json: &Value, toon: &Value) {
    assert_eq!(json, toon, "JSON and TOON decoded payloads diverged");
}

#[test]
fn sessions_list_json_and_toon_have_expected_shape() {
    let temp = TestConfigBuilder::new().build();
    seed_local_session(&temp, "https://example.com");

    let (json_out, json, toon_out, toon) = run_both(&temp, &["sessions", "list"]);
    assert_clean_stderr(&json_out);
    assert_clean_stderr(&toon_out);
    assert!(json.get("sessions").is_some());
    assert!(json.get("total").is_some());
    assert!(toon.get("sessions").is_some());
    assert!(toon.get("total").is_some());
    assert_json_toon_equivalent(&json, &toon);
}

#[test]
fn sessions_info_missing_json_and_toon_shape() {
    let temp = TestConfigBuilder::new().build();
    let (json_out, json, toon_out, toon) =
        run_both(&temp, &["sessions", "info", "https://example.com"]);
    assert_clean_stderr(&json_out);
    assert_clean_stderr(&toon_out);
    assert!(json.get("sessions").is_some());
    assert_eq!(json["total"], 0);
    assert!(toon.get("sessions").is_some());
    assert_eq!(toon["total"], 0);
}

#[test]
fn sessions_sync_empty_json_and_toon_shape() {
    let temp = TestConfigBuilder::new().build();
    let (json_out, json, toon_out, toon) = run_both(&temp, &["sessions", "sync"]);
    assert_clean_stderr(&json_out);
    assert_clean_stderr(&toon_out);
    assert_eq!(json["synced"], 0);
    assert_eq!(json["removed"], 0);
    assert_eq!(toon["synced"], 0);
    assert_eq!(toon["removed"], 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn query_json_and_toon_body_shape() {
    let app = Router::new().route(
        "/json",
        get(|| async { Json(serde_json::json!({"ok": true, "count": 2})) }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
            .unwrap();
    });

    let temp = TestConfigBuilder::new().build();
    let url = format!("http://{addr}/json");
    let (json_out, json, toon_out, toon) = run_both(&temp, &[&url]);
    assert_clean_stderr(&json_out);
    assert_clean_stderr(&toon_out);
    assert_eq!(json["ok"], true);
    assert_eq!(json["count"], 2);
    assert_eq!(toon["ok"], true);
    assert_eq!(toon["count"], 2);

    let _ = shutdown_tx.send(());
    let _ = server.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn services_json_and_toon_shapes() {
    let payload = serde_json::json!({
        "services": [
            {
                "id": "openai",
                "name": "OpenAI",
                "url": "https://openrouter.mpp.tempo.xyz",
                "serviceUrl": "https://openrouter.mpp.tempo.xyz/v1/chat/completions",
                "description": "LLM API",
                "categories": ["ai"],
                "methods": {"tempo": {"intents": ["charge"]}}
            }
        ]
    });

    let app = Router::new().route(
        "/services",
        get(move || {
            let p = payload.clone();
            async move { Json(p) }
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
            .unwrap();
    });

    let temp = TestConfigBuilder::new().build();
    let services_url = format!("http://{addr}/services");

    let mut cmd = test_command(&temp);
    let out_json = cmd
        .env("TEMPO_SERVICES_URL", &services_url)
        .args(["-j", "services", "list"])
        .output()
        .unwrap();
    assert!(out_json.status.success());
    assert_clean_stderr(&out_json);
    let json_list: Value = serde_json::from_str(String::from_utf8_lossy(&out_json.stdout).trim())
        .expect("valid services list json");
    assert!(json_list.as_array().is_some_and(|a| !a.is_empty()));

    let mut cmd = test_command(&temp);
    let out_toon = cmd
        .env("TEMPO_SERVICES_URL", &services_url)
        .args(["-t", "services", "list"])
        .output()
        .unwrap();
    assert!(out_toon.status.success());
    assert_clean_stderr(&out_toon);
    let toon_list: Value =
        toon_format::decode_default(String::from_utf8_lossy(&out_toon.stdout).trim())
            .expect("valid services list toon");
    assert!(toon_list.as_array().is_some_and(|a| !a.is_empty()));

    let mut cmd = test_command(&temp);
    let out_json = cmd
        .env("TEMPO_SERVICES_URL", &services_url)
        .args(["-j", "services", "info", "openai"])
        .output()
        .unwrap();
    assert!(out_json.status.success());
    assert_clean_stderr(&out_json);
    let json_info: Value = serde_json::from_str(String::from_utf8_lossy(&out_json.stdout).trim())
        .expect("valid services info json");
    assert_eq!(json_info["id"], "openai");

    let mut cmd = test_command(&temp);
    let out_toon = cmd
        .env("TEMPO_SERVICES_URL", &services_url)
        .args(["-t", "services", "info", "openai"])
        .output()
        .unwrap();
    assert!(out_toon.status.success());
    assert_clean_stderr(&out_toon);
    let toon_info: Value =
        toon_format::decode_default(String::from_utf8_lossy(&out_toon.stdout).trim())
            .expect("valid services info toon");
    assert_eq!(toon_info["id"], "openai");

    let _ = shutdown_tx.send(());
    let _ = server.await;
}
