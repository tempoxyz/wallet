//! Snapshot-like structure tests for JSON and TOON outputs.

use std::process::Output;

use axum::routing::get;
use axum::{Json, Router};
use serde_json::Value;

mod common;
use common::{test_command, TestConfigBuilder};

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
