use std::borrow::Cow;

#[tokio::main]
async fn main() {
    let id = std::env::var("ID")
        .unwrap_or(String::default())
        .parse::<u32>()
        .unwrap_or(u32::default());
    let config = match std::env::var("CONFIG_FILE") {
        Ok(config) => {
            println!("using zenoh config from file");
            zenoh::Config::from_file(config).unwrap_or_else(|_| {
                println!("can not use the config from the file");
                zenoh::Config::default()
            })
        }
        Err(_) => {
            println!("using default zenoh config");
            zenoh::Config::default()
        }
    };
    println!("cwd: {:?}", std::env::current_dir());
    //dbg!(config.clone());
    let session = zenoh::open(config).await.unwrap();
    let subscriber = session.declare_subscriber("ping").await.unwrap();
    while let Ok(sample) = subscriber.recv_async().await {
        println!(
            "Received: {:?}",
            sample
                .payload()
                .try_to_string()
                .unwrap_or(Cow::from("someone sent a bad message. contents"))
        );
        session
            .put("pong", format!("{} pong", id.clone()))
            .await
            .unwrap();
    }
}
