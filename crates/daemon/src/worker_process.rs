//! Spawn one worker with a private, platform-validated inherited channel.
//! The returned handles belong only to its coordinator, never another worker.
use std::{io, path::Path, process::Stdio};
use tokio::process::{Child, Command};

#[cfg(windows)]
pub(crate) type WorkerInput = tokio::net::windows::named_pipe::NamedPipeServer;
#[cfg(windows)]
pub(crate) type WorkerOutput = tokio::net::windows::named_pipe::NamedPipeServer;
#[cfg(not(windows))]
pub(crate) type WorkerInput = tokio::process::ChildStdin;
#[cfg(not(windows))]
pub(crate) type WorkerOutput = tokio::process::ChildStdout;

pub(crate) async fn spawn(path: &Path) -> io::Result<(Child, WorkerInput, WorkerOutput)> {
    let mut command = Command::new(path);
    command.stderr(Stdio::null()).kill_on_drop(true);
    #[cfg(windows)]
    {
        let pipes = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            veyra_ipc::windows::child_pipes(),
        )
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "worker pipe setup timeout"))??;
        // CREATE_NO_WINDOW: these console binaries are internal components, not
        // interactive terminals. Named-pipe servers and original client handles
        // stay non-inheritable. std::process serializes the temporary inheritable
        // stdio duplicates used by both worker spawns.
        command
            .creation_flags(0x0800_0000)
            .stdin(pipes.stdin)
            .stdout(pipes.stdout);
        let child = command.spawn()?;
        Ok((child, pipes.input, pipes.output))
    }
    #[cfg(not(windows))]
    {
        command.stdin(Stdio::piped()).stdout(Stdio::piped());
        let mut child = command.spawn()?;
        let input = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("worker stdin"))?;
        let output = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("worker stdout"))?;
        Ok((child, input, output))
    }
}
