//! Spec-alignment regression scenarios split from the main session integration file.

use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn top_up_challenge_not_found_refreshes_via_head_and_retries() {
    let rpc = SessionRpcServer::start().await;
    let server = SessionServer::start(SessionServerConfig {
        payee_mode: PayeeMode::Fixed,
        open_receipt_accepted: None,
        sse_voucher_flow: false,
        voucher_head_unsupported: false,
        sse_receipt_accepted: None,
        sse_required_cumulative: None,
        sse_reported_deposit: None,
        invalidating_problem_type_once: None,
        insufficient_balance_once: true,
        error_after_payment_once_status: None,
        response_delay_ms: 0,
    })
    .await;

    let temp = tempfile::TempDir::new().unwrap();
    setup_config_only(&temp, &rpc.base_url);

    let first_output = run_session_request(&temp, &server.url("/resource"));
    assert!(
        first_output.status.success(),
        "first request should seed reusable channel state: {}",
        get_combined_output(&first_output)
    );

    let before = server.snapshot();

    let second_output =
        run_session_request(&temp, &server.url("/resource-topup-challenge-not-found"));
    assert!(
        second_output.status.success(),
        "stale top-up challenge should recover via challenge refresh: {}",
        get_combined_output(&second_output)
    );

    let observed = server.snapshot();
    assert_eq!(
        observed.top_up_challenge_not_found_problem_count, 1,
        "server should emit one top-up challenge-not-found problem"
    );
    assert_eq!(
        observed.top_up_count,
        before.top_up_count + 2,
        "top-up should retry exactly once after challenge refresh"
    );
    assert!(
        observed.unauth_head_count > before.unauth_head_count,
        "challenge refresh for top-up recovery must use HEAD (got {} new HEAD requests)",
        observed.unauth_head_count - before.unauth_head_count
    );

    let channels = load_channels(&temp);
    assert_eq!(channels.len(), 1);
    assert_eq!(channels[0].state, "active");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn voucher_head_success_does_not_fallback_to_post() {
    let rpc = SessionRpcServer::start().await;
    let server = SessionServer::start(SessionServerConfig {
        payee_mode: PayeeMode::Fixed,
        open_receipt_accepted: None,
        sse_voucher_flow: true,
        voucher_head_unsupported: false,
        sse_receipt_accepted: Some(2_500_000),
        sse_required_cumulative: None,
        sse_reported_deposit: None,
        invalidating_problem_type_once: None,
        insufficient_balance_once: false,
        error_after_payment_once_status: None,
        response_delay_ms: 0,
    })
    .await;

    let temp = tempfile::TempDir::new().unwrap();
    setup_config_only(&temp, &rpc.base_url);

    let first_output = run_session_request(&temp, &server.url("/stream"));
    assert!(
        first_output.status.success(),
        "first request should establish session state: {}",
        get_combined_output(&first_output)
    );

    let before = server.snapshot();

    let second_output = run_session_request(&temp, &server.url("/stream"));
    assert!(
        second_output.status.success(),
        "HEAD voucher transport success path should keep stream healthy: {}",
        get_combined_output(&second_output)
    );

    let observed = server.snapshot();
    assert_eq!(
        observed.voucher_head_updates,
        before.voucher_head_updates + 1,
        "voucher update should use HEAD transport once"
    );
    assert_eq!(
        observed.voucher_head_statuses.last().copied(),
        Some(200),
        "voucher HEAD should succeed without requiring fallback"
    );
    assert_eq!(
        observed.voucher_post_updates, before.voucher_post_updates,
        "successful HEAD voucher update must not trigger unnecessary POST fallback"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failure_response_payment_receipt_is_not_treated_as_success() {
    let rpc = SessionRpcServer::start().await;
    let server = SessionServer::start(SessionServerConfig {
        payee_mode: PayeeMode::Fixed,
        open_receipt_accepted: None,
        sse_voucher_flow: false,
        voucher_head_unsupported: false,
        sse_receipt_accepted: None,
        sse_required_cumulative: None,
        sse_reported_deposit: None,
        invalidating_problem_type_once: None,
        insufficient_balance_once: false,
        error_after_payment_once_status: Some(500),
        response_delay_ms: 0,
    })
    .await;

    let temp = tempfile::TempDir::new().unwrap();
    setup_config_only(&temp, &rpc.base_url);

    let first_output = run_session_request(&temp, &server.url("/resource"));
    assert!(
        first_output.status.success(),
        "first request should establish reusable channel state: {}",
        get_combined_output(&first_output)
    );

    let second_output = run_session_request(&temp, &server.url("/resource-error-with-receipt"));
    assert!(
        !second_output.status.success(),
        "failure response carrying Payment-Receipt must still fail paid request"
    );

    let channels = load_channels(&temp);
    assert_eq!(channels.len(), 1);
    assert_eq!(
        channels[0].cumulative_amount,
        SESSION_AMOUNT * 2,
        "error response Payment-Receipt must not advance local paid cumulative state"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn endpoint_switch_reuses_channel_and_targets_current_path_for_updates() {
    let rpc = SessionRpcServer::start().await;
    let server = SessionServer::start(SessionServerConfig {
        payee_mode: PayeeMode::Fixed,
        open_receipt_accepted: None,
        sse_voucher_flow: true,
        voucher_head_unsupported: true,
        sse_receipt_accepted: Some(2_200_000),
        sse_required_cumulative: Some(2_000_000),
        sse_reported_deposit: Some(1_000_000),
        invalidating_problem_type_once: None,
        insufficient_balance_once: false,
        error_after_payment_once_status: None,
        response_delay_ms: 0,
    })
    .await;

    let temp = tempfile::TempDir::new().unwrap();
    setup_config_only(&temp, &rpc.base_url);

    let first_output = run_session_request(&temp, &server.url("/stream-a"));
    assert!(
        first_output.status.success(),
        "endpoint A stream should establish reusable channel state: {}",
        get_combined_output(&first_output)
    );

    let before = server.snapshot();

    let second_output = run_session_request(&temp, &server.url("/stream-b"));
    assert!(
        second_output.status.success(),
        "endpoint B stream should reuse channel and complete voucher/top-up flow: {}",
        get_combined_output(&second_output)
    );

    let observed = server.snapshot();
    assert_eq!(
        observed.open_count, before.open_count,
        "endpoint B should reuse the existing channel instead of opening a new one"
    );

    let voucher_paths = &observed.voucher_paths[before.voucher_paths.len()..];
    assert!(
        !voucher_paths.is_empty() && voucher_paths.iter().all(|p| p == "/stream-b"),
        "all endpoint-B voucher submissions should target endpoint B: {voucher_paths:?}"
    );

    let top_up_paths = &observed.top_up_paths[before.top_up_paths.len()..];
    assert!(
        !top_up_paths.is_empty() && top_up_paths.iter().all(|p| p == "/stream-b"),
        "all endpoint-B top-up submissions should target endpoint B: {top_up_paths:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn new_session_while_prior_stream_active_recovers_without_state_corruption() {
    let rpc = SessionRpcServer::start().await;
    let server = SessionServer::start(SessionServerConfig {
        payee_mode: PayeeMode::Fixed,
        open_receipt_accepted: None,
        sse_voucher_flow: true,
        voucher_head_unsupported: false,
        sse_receipt_accepted: None,
        sse_required_cumulative: None,
        sse_reported_deposit: None,
        invalidating_problem_type_once: None,
        insufficient_balance_once: false,
        error_after_payment_once_status: None,
        response_delay_ms: 0,
    })
    .await;

    let temp = tempfile::TempDir::new().unwrap();
    setup_config_only(&temp, &rpc.base_url);

    let mut stalled = spawn_session_request_with_env(
        &temp,
        &server.url("/stream-stall"),
        &[
            ("TEMPO_SESSION_MAX_VOUCHER_RETRIES", "100"),
            ("TEMPO_SESSION_STALL_TIMEOUT_MS", "1000"),
            ("TEMPO_SESSION_NORMAL_TIMEOUT_MS", "5000"),
        ],
    );
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert!(
        stalled.try_wait().unwrap().is_none(),
        "stalled stream should remain active while retry timer is running"
    );

    let recovery_output = std::thread::scope(|scope| {
        let join = scope.spawn(|| run_session_request(&temp, &server.url("/resource")));
        std::thread::sleep(std::time::Duration::from_millis(250));
        let _ = stalled.kill();
        let _ = stalled.wait();
        join.join().unwrap()
    });
    assert!(
        recovery_output.status.success(),
        "new session request started during active stream should recover cleanly: {}",
        get_combined_output(&recovery_output)
    );

    let follow_up = run_session_request(&temp, &server.url("/resource"));
    assert!(
        follow_up.status.success(),
        "follow-up request after stream interruption should remain healthy: {}",
        get_combined_output(&follow_up)
    );

    let observed = server.snapshot();
    assert_eq!(
        observed.open_count, 1,
        "active-stream replacement handling should not corrupt state into duplicate opens"
    );

    let channels = load_channels(&temp);
    assert_eq!(
        channels.len(),
        1,
        "active stream replacement path should preserve a single persisted channel row"
    );
    assert_eq!(channels[0].state, "active");
    assert!(
        channels[0].cumulative_amount >= SESSION_AMOUNT * 2,
        "recovery path should keep cumulative progress monotonic"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fee_payer_variants_cover_open_and_top_up_flows() {
    let rpc = SessionRpcServer::start().await;
    let server = SessionServer::start(SessionServerConfig {
        payee_mode: PayeeMode::Fixed,
        open_receipt_accepted: None,
        sse_voucher_flow: true,
        voucher_head_unsupported: true,
        sse_receipt_accepted: Some(2_500_000),
        sse_required_cumulative: Some(2_500_000),
        sse_reported_deposit: Some(500_000),
        invalidating_problem_type_once: None,
        insufficient_balance_once: false,
        error_after_payment_once_status: None,
        response_delay_ms: 0,
    })
    .await;

    let temp_true = tempfile::TempDir::new().unwrap();
    setup_config_only(&temp_true, &rpc.base_url);
    let before_true = server.snapshot();
    let true_seed = run_session_request(&temp_true, &server.url("/stream-fee-payer-true"));
    assert!(
        true_seed.status.success(),
        "feePayer=true seed request should establish reusable session state: {}",
        get_combined_output(&true_seed)
    );
    let true_output = run_session_request(&temp_true, &server.url("/stream-fee-payer-true"));
    assert!(
        true_output.status.success(),
        "feePayer=true session flow should perform voucher/top-up successfully: {}",
        get_combined_output(&true_output)
    );
    let after_true = server.snapshot();
    assert_eq!(
        after_true.open_count,
        before_true.open_count + 1,
        "feePayer=true path should perform one open"
    );
    assert!(
        after_true.top_up_count > before_true.top_up_count,
        "feePayer=true path should perform top-up flow"
    );

    let temp_false = tempfile::TempDir::new().unwrap();
    setup_config_only(&temp_false, &rpc.base_url);
    let before_false = server.snapshot();
    let false_seed = run_session_request(&temp_false, &server.url("/stream-fee-payer-false"));
    assert!(
        false_seed.status.success(),
        "feePayer=false seed request should establish reusable session state: {}",
        get_combined_output(&false_seed)
    );
    let false_output = run_session_request(&temp_false, &server.url("/stream-fee-payer-false"));
    assert!(
        false_output.status.success(),
        "feePayer=false session flow should perform voucher/top-up successfully: {}",
        get_combined_output(&false_output)
    );
    let after_false = server.snapshot();
    assert_eq!(
        after_false.open_count,
        before_false.open_count + 1,
        "feePayer=false path should perform one open"
    );
    assert!(
        after_false.top_up_count > before_false.top_up_count,
        "feePayer=false path should perform top-up flow"
    );
}
