mod app;
mod signaling;
mod ui;
mod webrtc;

#[tokio::main]
async fn main() {
    app::start_server().await;
}
