use std::time::Duration;
use veyra_connectivity::TransportConfig;
use veyra_transport::{TransportError, WebRtcPeer};

#[tokio::test]
async fn received_data_does_not_confuse_open_notification_with_close() {
    let config = TransportConfig {
        bind_addrs: vec!["127.0.0.1:0".into()],
        ..Default::default()
    };
    let a = WebRtcPeer::new(config.clone()).await.unwrap();
    let b = WebRtcPeer::new(config).await.unwrap();
    let offer = a.offer().await.unwrap();
    let answer = b.answer(offer).await.unwrap();
    a.accept_answer(answer).await.unwrap();
    let (left, right) = tokio::join!(a.wait_open(), b.wait_open());
    left.unwrap();
    right.unwrap();
    b.send(b"synthetic lifecycle probe").await.unwrap();
    assert_eq!(a.recv().await.unwrap(), b"synthetic lifecycle probe");
    let receive = a.recv();
    let close = async {
        tokio::task::yield_now().await;
        a.close().await.unwrap();
    };
    let (received, ()) = tokio::join!(tokio::time::timeout(Duration::from_secs(1), receive), close);
    assert_eq!(received.unwrap(), Err(TransportError::Closed));
    b.close().await.unwrap();
}
