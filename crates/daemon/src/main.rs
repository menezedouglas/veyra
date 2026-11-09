#[tokio::main]
async fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    if veyra_daemon::network_host::run().await.is_err() {
        std::process::exit(78);
    }
}
