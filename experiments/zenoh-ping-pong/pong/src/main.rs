use std::time::Duration;

#[tokio::main]
async fn main() {
    let id = std::env::var("ID")
        .unwrap_or(String::default())
        .parse::<u32>()
        .unwrap_or(u32::default());
    let config = match std::env::var("CONFIG_FILE") {
        Ok(config) => {
            println!("using zenoh config from file");
            zenoh::Config::from_file(config).unwrap_or(zenoh::Config::default())
        }
        Err(_) => {
            println!("using default zenoh config");
            zenoh::Config::default()
        }
    };
    let session = zenoh::open(config).await.unwrap();
    let subscriber = session.declare_subscriber("pong").await.unwrap();
    while let Ok(sample) = subscriber.recv_async().await {
        println!("Received: {:?}", sample);
        tokio::time::sleep(Duration::from_secs(1)).await;
        session
            .put("ping", format!("{} ping", id.clone()))
            .await
            .unwrap();
    }
}
