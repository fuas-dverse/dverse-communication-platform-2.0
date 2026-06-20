use std::time::Duration;

use a2a::message::{A2AMessage, AGENT_A_INBOX, AGENT_A_NAME, AGENT_B_INBOX, AGENT_B_NAME};
use tokio::time::timeout;

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
async fn two_turn_exchange_completes() {
    let (sess_a, sess_b) = make_peer_pair(17910).await;

    let sub_a = sess_a.declare_subscriber(AGENT_A_INBOX).await.unwrap();
    let sub_b = sess_b.declare_subscriber(AGENT_B_INBOX).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    // A sends turn 1 to B
    let msg_a = A2AMessage::new(AGENT_A_NAME, AGENT_B_NAME, "hello from A".to_string(), 1);
    sess_a
        .put(AGENT_B_INBOX, serde_json::to_vec(&msg_a).unwrap())
        .await
        .unwrap();

    // B receives
    let sample_b = timeout(Duration::from_secs(5), sub_b.recv_async())
        .await
        .expect("B did not receive A's message")
        .unwrap();
    let received_by_b: A2AMessage =
        serde_json::from_slice(&sample_b.payload().to_bytes()).unwrap();

    // B replies to A
    let reply_b = A2AMessage::new(
        AGENT_B_NAME,
        AGENT_A_NAME,
        "hello from B".to_string(),
        received_by_b.turn,
    );
    sess_b
        .put(AGENT_A_INBOX, serde_json::to_vec(&reply_b).unwrap())
        .await
        .unwrap();

    // A receives B's reply
    let sample_a = timeout(Duration::from_secs(5), sub_a.recv_async())
        .await
        .expect("A did not receive B's reply")
        .unwrap();
    let received_by_a: A2AMessage =
        serde_json::from_slice(&sample_a.payload().to_bytes()).unwrap();

    assert_eq!(received_by_a.from, AGENT_B_NAME);
    assert_eq!(received_by_a.content, "hello from B");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn turn_numbers_are_correct() {
    let (sess_a, sess_b) = make_peer_pair(17911).await;

    let sub_b = sess_b.declare_subscriber(AGENT_B_INBOX).await.unwrap();
    let sub_a = sess_a.declare_subscriber(AGENT_A_INBOX).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    let sent_turn = 3u32;
    let msg = A2AMessage::new(AGENT_A_NAME, AGENT_B_NAME, "query".to_string(), sent_turn);
    sess_a
        .put(AGENT_B_INBOX, serde_json::to_vec(&msg).unwrap())
        .await
        .unwrap();

    let received: A2AMessage = serde_json::from_slice(
        &timeout(Duration::from_secs(5), sub_b.recv_async())
            .await
            .unwrap()
            .unwrap()
            .payload()
            .to_bytes(),
    )
    .unwrap();

    // B replies echoing the same turn number
    let reply = A2AMessage::new(AGENT_B_NAME, AGENT_A_NAME, "answer".to_string(), received.turn);
    sess_b
        .put(AGENT_A_INBOX, serde_json::to_vec(&reply).unwrap())
        .await
        .unwrap();

    let final_msg: A2AMessage = serde_json::from_slice(
        &timeout(Duration::from_secs(5), sub_a.recv_async())
            .await
            .unwrap()
            .unwrap()
            .payload()
            .to_bytes(),
    )
    .unwrap();

    assert_eq!(final_msg.turn, sent_turn);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn from_to_fields_correct() {
    let (sess_a, sess_b) = make_peer_pair(17912).await;

    let sub_b = sess_b.declare_subscriber(AGENT_B_INBOX).await.unwrap();
    let sub_a = sess_a.declare_subscriber(AGENT_A_INBOX).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    // A → B
    let msg = A2AMessage::new(AGENT_A_NAME, AGENT_B_NAME, "request".to_string(), 1);
    sess_a
        .put(AGENT_B_INBOX, serde_json::to_vec(&msg).unwrap())
        .await
        .unwrap();

    let ab: A2AMessage = serde_json::from_slice(
        &timeout(Duration::from_secs(5), sub_b.recv_async())
            .await
            .unwrap()
            .unwrap()
            .payload()
            .to_bytes(),
    )
    .unwrap();
    assert_eq!(ab.from, AGENT_A_NAME);
    assert_eq!(ab.to, AGENT_B_NAME);

    // B → A
    let reply = A2AMessage::new(AGENT_B_NAME, AGENT_A_NAME, "response".to_string(), 1);
    sess_b
        .put(AGENT_A_INBOX, serde_json::to_vec(&reply).unwrap())
        .await
        .unwrap();

    let ba: A2AMessage = serde_json::from_slice(
        &timeout(Duration::from_secs(5), sub_a.recv_async())
            .await
            .unwrap()
            .unwrap()
            .payload()
            .to_bytes(),
    )
    .unwrap();
    assert_eq!(ba.from, AGENT_B_NAME);
    assert_eq!(ba.to, AGENT_A_NAME);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn timeout_path() {
    let (sess_a, _sess_b) = make_peer_pair(17913).await;

    let sub_a = sess_a.declare_subscriber(AGENT_A_INBOX).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    // A sends to B but B never replies
    let msg = A2AMessage::new(AGENT_A_NAME, AGENT_B_NAME, "question".to_string(), 1);
    sess_a
        .put(AGENT_B_INBOX, serde_json::to_vec(&msg).unwrap())
        .await
        .unwrap();

    // Short timeout simulates the 60s select! in run_council
    let result = timeout(Duration::from_millis(300), sub_a.recv_async()).await;
    assert!(result.is_err(), "A's inbox should time out — B never replied");
}
