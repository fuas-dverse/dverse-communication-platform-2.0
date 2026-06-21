use std::time::Duration;

use a2a::message::{A2AMessage, AGENT_A_INBOX, AGENT_B_INBOX};
use tokio::time::timeout;

/// Open two peer sessions: `a` listens on `port`, `b` connects to it.
/// Multicast scouting is disabled so tests are deterministic and self-contained.
async fn make_peer_pair(port: u16) -> (zenoh::Session, zenoh::Session) {
    let mut cfg_a = zenoh::Config::default();
    cfg_a.insert_json5("mode", "\"peer\"").unwrap();
    cfg_a
        .insert_json5(
            "listen/endpoints",
            &format!(r#"["tcp/127.0.0.1:{port}"]"#),
        )
        .unwrap();
    cfg_a
        .insert_json5("scouting/multicast/enabled", "false")
        .unwrap();

    let mut cfg_b = zenoh::Config::default();
    cfg_b.insert_json5("mode", "\"peer\"").unwrap();
    cfg_b
        .insert_json5(
            "connect/endpoints",
            &format!(r#"["tcp/127.0.0.1:{port}"]"#),
        )
        .unwrap();
    cfg_b
        .insert_json5("scouting/multicast/enabled", "false")
        .unwrap();

    let sess_a = zenoh::open(cfg_a).await.unwrap();
    let sess_b = zenoh::open(cfg_b).await.unwrap();
    (sess_a, sess_b)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn message_published_to_correct_topic() {
    let (sess_a, sess_b) = make_peer_pair(17900).await;

    let sub = sess_b.declare_subscriber(AGENT_B_INBOX).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    let msg = A2AMessage::new("agent-a", "agent-b", "hello".to_string(), 1);
    sess_a
        .put(AGENT_B_INBOX, serde_json::to_vec(&msg).unwrap())
        .await
        .unwrap();

    let received = timeout(Duration::from_secs(5), sub.recv_async())
        .await
        .expect("timeout waiting for message")
        .unwrap();

    assert_eq!(received.key_expr().as_str(), AGENT_B_INBOX);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn deserialized_message_matches_original() {
    let (sess_a, sess_b) = make_peer_pair(17901).await;

    let sub = sess_b.declare_subscriber(AGENT_B_INBOX).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    let msg = A2AMessage::new("agent-a", "agent-b", "test content".to_string(), 2);
    sess_a
        .put(AGENT_B_INBOX, serde_json::to_vec(&msg).unwrap())
        .await
        .unwrap();

    let sample = timeout(Duration::from_secs(5), sub.recv_async())
        .await
        .expect("timeout")
        .unwrap();

    let bytes = sample.payload().to_bytes();
    let decoded: A2AMessage = serde_json::from_slice(&bytes).unwrap();

    assert_eq!(decoded.from, "agent-a");
    assert_eq!(decoded.to, "agent-b");
    assert_eq!(decoded.content, "test content");
    assert_eq!(decoded.turn, 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn invalid_payload_does_not_panic() {
    let (sess_a, sess_b) = make_peer_pair(17902).await;

    let sub = sess_b.declare_subscriber(AGENT_B_INBOX).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    sess_a
        .put(AGENT_B_INBOX, b"not-json".to_vec())
        .await
        .unwrap();

    let sample = timeout(Duration::from_secs(5), sub.recv_async())
        .await
        .expect("timeout")
        .unwrap();

    let bytes = sample.payload().to_bytes();
    let result: Result<A2AMessage, _> = serde_json::from_slice(&bytes);
    assert!(result.is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn reply_routed_to_correct_inbox() {
    let (sess_a, sess_b) = make_peer_pair(17903).await;

    let sub_a = sess_a.declare_subscriber(AGENT_A_INBOX).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    let reply = A2AMessage::new("agent-b", "agent-a", "reply from B".to_string(), 1);
    sess_b
        .put(AGENT_A_INBOX, serde_json::to_vec(&reply).unwrap())
        .await
        .unwrap();

    let sample = timeout(Duration::from_secs(5), sub_a.recv_async())
        .await
        .expect("timeout")
        .unwrap();

    let bytes = sample.payload().to_bytes();
    let decoded: A2AMessage = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(decoded.from, "agent-b");
    assert_eq!(decoded.to, "agent-a");
}
