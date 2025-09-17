use futures_util::{pin_mut, SinkExt, StreamExt};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ws_url = "ws://127.0.0.1:9000/ws/data";

    println!("Connecting to {}...", ws_url);
    let (ws_stream, _) = connect_async(ws_url).await.expect("Failed to connect");
    println!("WebSocket connection established.");

    let (mut write, read) = ws_stream.split();

    // Manually construct the JSON string to avoid serialization/import issues.
    // This is the format the daemon expects, based on its internal tests.
    let sub_msg_json = r#"{"type":"subscribe","topic":"eeg_voltage","epoch":1}"#.to_string();

    println!("Sending subscription message: {}", sub_msg_json);
    write.send(Message::Text(sub_msg_json)).await?;
    println!("Subscription message sent. Waiting for data...");

    let read_future = read.for_each(|message| async {
        match message {
            Ok(msg) => println!("Received: {}", msg),
            Err(e) => eprintln!("WebSocket error: {}", e),
        }
    });

    pin_mut!(read_future);
    read_future.await;

    Ok(())
}
