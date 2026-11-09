fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "start_session",
            "create_invite",
            "join_invite",
            "verify_peer",
            "send_message",
            "list_pending",
            "open_once",
            "discard_message",
            "revoke_peer",
            "connection_status",
            "end_session",
        ]),
    ))
    .expect("Tauri build configuration");
}
