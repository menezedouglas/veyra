#![cfg(windows)]
//! Real Windows processes and OS channels; all identities and data are fixtures.
use std::{path::Path, process::Stdio, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
    sync::Mutex,
    time::timeout,
};
use veyra_daemon::{
    DaemonClient, VaultClient,
    wire::{Command as NetworkCommand, Reply},
};
use veyra_ipc::{VaultRequest as Request, VaultResponse as Response};
use veyra_protocol::{Frame, IdentityBinding};

// Test-only calls to documented Win32 process lifecycle APIs. This test never
// changes or suspends an unrelated process and needs no additional dependency.
#[link(name = "kernel32")]
unsafe extern "system" {
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut std::ffi::c_void;
    fn TerminateProcess(process: *mut std::ffi::c_void, exit_code: u32) -> i32;
    fn WaitForSingleObject(process: *mut std::ffi::c_void, milliseconds: u32) -> u32;
}

async fn binding(vault: &Arc<Mutex<VaultClient>>) -> IdentityBinding {
    let Response::Identity { binding, .. } = vault
        .lock()
        .await
        .request(&Request::Status {})
        .await
        .unwrap()
    else {
        panic!("fixture identity response");
    };
    let Frame::IdentityBinding(binding) = Frame::decode(&binding).unwrap() else {
        panic!("fixture identity binding");
    };
    binding
}

#[tokio::test]
async fn windows_workers_refuse_anonymous_stdio_before_bootstrap() {
    for executable in [
        env!("CARGO_BIN_EXE_veyra-vault"),
        env!("CARGO_BIN_EXE_veyra-daemon"),
    ] {
        let mut child = Command::new(executable)
            .creation_flags(0x0800_0000)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let result = timeout(Duration::from_secs(5), child.wait())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result.code(), Some(78));
        let mut output = Vec::new();
        child
            .stdout
            .take()
            .unwrap()
            .read_to_end(&mut output)
            .await
            .unwrap();
        assert!(output.is_empty());
    }
}

#[tokio::test]
async fn windows_worker_exits_without_protocol_output_on_partial_bootstrap() {
    let pipes = veyra_ipc::windows::child_pipes().await.unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_veyra-vault"))
        .creation_flags(0x0800_0000)
        .stdin(pipes.stdin)
        .stdout(pipes.stdout)
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut input = pipes.input;
    input.write_all(&[1, 2, 3]).await.unwrap();
    drop(input);
    let result = timeout(Duration::from_secs(5), child.wait())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.code(), Some(78));
    let mut output = pipes.output;
    let mut bytes = Vec::new();
    timeout(Duration::from_secs(5), output.read_to_end(&mut bytes))
        .await
        .unwrap()
        .unwrap();
    assert!(bytes.is_empty());
}

#[tokio::test]
async fn windows_crashed_vault_invalidates_its_channel_without_restarting() {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    let mut vault = VaultClient::spawn(Path::new(env!("CARGO_BIN_EXE_veyra-vault")))
        .await
        .unwrap();
    let pid = vault.process_id().unwrap();
    // PROCESS_TERMINATE | SYNCHRONIZE, with inheritance explicitly disabled.
    let handle = unsafe { OpenProcess(0x0010_0001, 0, pid) };
    assert!(!handle.is_null());
    // SAFETY: OpenProcess returned an owned handle; it is closed once by RAII.
    let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
    // Confirm the original worker is still responding after opening its handle.
    // If it had died and its PID were reused before OpenProcess, this fails
    // before any termination call can affect that unrelated process.
    assert!(matches!(
        vault.request(&Request::Status {}).await.unwrap(),
        Response::Identity { .. }
    ));
    assert_ne!(unsafe { TerminateProcess(handle.as_raw_handle(), 77) }, 0);
    assert_eq!(
        unsafe { WaitForSingleObject(handle.as_raw_handle(), 5000) },
        0
    );
    assert!(vault.request(&Request::Status {}).await.is_err());
    assert!(vault.process_id().is_none());
    assert!(vault.request(&Request::Status {}).await.is_err());
    vault.end().await;
}

#[tokio::test]
async fn windows_daemon_and_vault_exchange_only_public_binding_through_coordinator() {
    let vault = Arc::new(Mutex::new(
        VaultClient::spawn(Path::new(env!("CARGO_BIN_EXE_veyra-vault")))
            .await
            .unwrap(),
    ));
    let original = binding(&vault).await;
    let mut daemon =
        DaemonClient::spawn(Path::new(env!("CARGO_BIN_EXE_veyra-daemon")), vault.clone())
            .await
            .unwrap();
    // Runtime initialization exercises real custody IPC without opening WSS.
    assert!(matches!(
        daemon
            .request(NetworkCommand::Start {
                endpoint: "wss://synthetic-operator.invalid/v1/rendezvous".into(),
                transport: Default::default(),
            })
            .await
            .unwrap(),
        Reply::Ok {}
    ));
    let current = binding(&vault).await;
    assert_eq!(original.identity, current.identity);
    assert_eq!(original.runtime_id, current.runtime_id);
    assert_ne!(original.noise_public, current.noise_public);
    veyra_identity::verify_binding(&current, &current.noise_public).unwrap();
    assert!(
        matches!(daemon.request(NetworkCommand::Peers {}).await.unwrap(), Reply::Peers { peers } if peers.is_empty())
    );
    // A network worker cannot silently replace its bound transport key.
    assert!(matches!(
        vault
            .lock()
            .await
            .request(&Request::BindTransport {
                noise_public: [9; 32]
            })
            .await
            .unwrap(),
        Response::Refused { .. }
    ));
    daemon.end().await;
    assert_eq!(binding(&vault).await, current);
    // Dropping the bridge releases its final custody reference asynchronously.
    timeout(Duration::from_secs(5), async {
        while Arc::strong_count(&vault) != 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    Arc::try_unwrap(vault)
        .ok()
        .unwrap()
        .into_inner()
        .end()
        .await;
}
