fn main() -> anyhow::Result<()> {
    threadbridge_rust::app_server_ws_worker::run_from_env()
}
