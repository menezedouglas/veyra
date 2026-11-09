//! Preconnected Windows named pipes, delivered only as the child's standard
//! handles. Names are random and never appear in argv, environment or files.
//! The server rejects remote clients; only this logon SID has data access.
//!
//! The ACL uses individual rights: FILE_GENERIC_WRITE would also grant
//! FILE_CREATE_PIPE_INSTANCE. See Microsoft's Named Pipe Security and Access
//! Rights documentation. The challenge over these inherited handles remains the
//! instance authentication; process IDs are an additional OS ownership check.
use std::{
    ffi::c_void,
    fs::{File, OpenOptions},
    io,
    mem::{size_of, size_of_val},
    os::windows::{
        fs::OpenOptionsExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    process::Stdio,
    ptr,
};
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use windows_sys::Win32::{
    Foundation::{
        GetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE, LocalFree,
        SetHandleInformation,
    },
    Security::{
        ACCESS_ALLOWED_ACE, ACE_HEADER, ACL,
        Authorization::{
            ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
            GetSecurityInfo, SDDL_REVISION_1, SE_KERNEL_OBJECT,
        },
        DACL_SECURITY_INFORMATION, GetAce, GetSecurityDescriptorControl, GetTokenInformation,
        OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, SE_DACL_PROTECTED,
        SECURITY_ATTRIBUTES, TOKEN_GROUPS, TOKEN_INFORMATION_CLASS, TOKEN_QUERY, TOKEN_USER,
        TokenGroups, TokenUser,
    },
    Storage::FileSystem::{
        FILE_GENERIC_READ, FILE_READ_ATTRIBUTES, FILE_TYPE_PIPE, FILE_WRITE_DATA, GetFileType,
        READ_CONTROL, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT, SYNCHRONIZE,
    },
    System::{
        Console::{GetStdHandle, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE},
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        },
        Pipes::{
            GetNamedPipeClientProcessId, GetNamedPipeInfo, GetNamedPipeServerProcessId,
            GetNamedPipeServerSessionId, PIPE_SERVER_END, PIPE_TYPE_MESSAGE,
        },
        RemoteDesktop::ProcessIdToSessionId,
        SystemServices::{ACCESS_ALLOWED_ACE_TYPE, SE_GROUP_LOGON_ID},
        Threading::{GetCurrentProcess, GetCurrentProcessId, OpenProcessToken},
    },
};

// Data, EA and attribute rights plus READ_CONTROL/SYNCHRONIZE, without append
// (FILE_CREATE_PIPE_INSTANCE), WRITE_DAC, WRITE_OWNER or generic rights.
const PIPE_LOGON_ACCESS: u32 = 0x0012_019b;
const CHILD_WRITE_ACCESS: u32 = FILE_WRITE_DATA | FILE_READ_ATTRIBUTES | READ_CONTROL | SYNCHRONIZE;

/// The coordinator owns the asynchronous server ends. The child ends are
/// non-inheritable. std::process serializes creation of temporary inheritable
/// stdio duplicates during spawn; it does not supply an explicit handle list.
pub struct ChildPipes {
    pub stdin: Stdio,
    pub stdout: Stdio,
    pub input: NamedPipeServer,
    pub output: NamedPipeServer,
}

/// Create private, connected handles before spawning a worker. There is never
/// a reconnect/accept loop and each pipe permits exactly one instance/client.
pub async fn child_pipes() -> io::Result<ChildPipes> {
    let (input, child_stdin, output, child_stdout) = {
        let identity = LogonIdentity::current()?;
        let descriptor = identity.descriptor()?;
        let (input, child_stdin) = connected_pipe(&identity, &descriptor, true)?;
        let (output, child_stdout) = connected_pipe(&identity, &descriptor, false)?;
        (input, child_stdin, output, child_stdout)
    };
    // Drop the immutable raw security-descriptor allocation before any await.
    // Only owned Send handles remain in the coordinator's asynchronous future.
    input.connect().await?;
    output.connect().await?;
    Ok(ChildPipes {
        stdin: Stdio::from(child_stdin),
        stdout: Stdio::from(child_stdout),
        input,
        output,
    })
}

fn connected_pipe(
    identity: &LogonIdentity,
    descriptor: &LocalAllocation,
    parent_writes: bool,
) -> io::Result<(NamedPipeServer, File)> {
    let name = random_pipe_name()?;
    let server = create_server(&name, descriptor, parent_writes)?;
    let client = open_client(&name, parent_writes)?;
    let raw = server.as_raw_handle() as HANDLE;
    let mut client_pid = 0;
    // The parent opened the client end before any worker existed. Reject an
    // unexpected process even if it won a same-logon race for the random name.
    if unsafe { GetNamedPipeClientProcessId(raw, &mut client_pid) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if client_pid != unsafe { GetCurrentProcessId() } {
        return Err(denied());
    }
    validate_security(raw, identity)?;
    ensure_not_inherited(raw)?;
    ensure_not_inherited(client.as_raw_handle() as HANDLE)?;
    Ok((server, client))
}

fn create_server(
    name: &str,
    descriptor: &LocalAllocation,
    parent_writes: bool,
) -> io::Result<NamedPipeServer> {
    let mut attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: 0,
    };
    let mut options = ServerOptions::new();
    options
        .access_inbound(!parent_writes)
        .access_outbound(parent_writes)
        .first_pipe_instance(true)
        .reject_remote_clients(true)
        .max_instances(1)
        .in_buffer_size(crate::MAX_IPC_FRAME as u32)
        .out_buffer_size(crate::MAX_IPC_FRAME as u32);
    // SAFETY: descriptor and attributes remain valid for the synchronous create
    // call. Windows copies the security descriptor into the new pipe object.
    let server = unsafe {
        options.create_with_security_attributes_raw(
            name,
            (&mut attributes as *mut SECURITY_ATTRIBUTES).cast(),
        )?
    };
    if unsafe { SetHandleInformation(server.as_raw_handle() as HANDLE, HANDLE_FLAG_INHERIT, 0) }
        == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(server)
}

fn open_client(name: &str, parent_writes: bool) -> io::Result<File> {
    OpenOptions::new()
        .access_mode(if parent_writes {
            FILE_GENERIC_READ
        } else {
            CHILD_WRITE_ACCESS
        })
        // The parent never needs to impersonate the child or another principal.
        .custom_flags(SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION)
        .open(name)
}

fn random_pipe_name() -> io::Result<String> {
    let mut random = [0; 32];
    getrandom::getrandom(&mut random)
        .map_err(|_| io::Error::other("IPC randomness unavailable"))?;
    let mut name = String::from(r"\\.\pipe\veyra-");
    for byte in random {
        use std::fmt::Write;
        write!(&mut name, "{byte:02x}").map_err(|_| io::Error::other("IPC name unavailable"))?;
    }
    Ok(name)
}

/// Check the inherited client handles before reading any bootstrap material.
/// A regular redirected file, anonymous pipe, broad/default ACL, other logon or
/// pipe belonging to a different server process is refused.
pub fn validate_inherited_stdio() -> io::Result<()> {
    let identity = LogonIdentity::current()?;
    let parent = parent_process_id()?;
    let mut session = 0;
    if unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut session) } == 0 {
        return Err(io::Error::last_os_error());
    }
    for stream in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE] {
        let handle = unsafe { GetStdHandle(stream) };
        validate_client(handle, &identity, parent, session)?;
    }
    Ok(())
}

fn validate_client(
    handle: HANDLE,
    identity: &LogonIdentity,
    parent: u32,
    session: u32,
) -> io::Result<()> {
    if handle.is_null()
        || handle == INVALID_HANDLE_VALUE
        || unsafe { GetFileType(handle) } != FILE_TYPE_PIPE
    {
        return Err(denied());
    }
    let mut flags = 0;
    let mut instances = 0;
    if unsafe {
        GetNamedPipeInfo(
            handle,
            &mut flags,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut instances,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if flags & (PIPE_SERVER_END | PIPE_TYPE_MESSAGE) != 0 || instances != 1 {
        return Err(denied());
    }
    let mut server_pid = 0;
    let mut server_session = 0;
    if unsafe { GetNamedPipeServerProcessId(handle, &mut server_pid) } == 0
        || unsafe { GetNamedPipeServerSessionId(handle, &mut server_session) } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if parent == 0 || server_pid != parent || server_session != session {
        return Err(denied());
    }
    validate_security(handle, identity)
}

fn parent_process_id() -> io::Result<u32> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    let snapshot = owned_handle(snapshot)?;
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let pid = unsafe { GetCurrentProcessId() };
    let mut found = unsafe { Process32FirstW(snapshot.as_raw_handle() as HANDLE, &mut entry) };
    while found != 0 {
        if entry.th32ProcessID == pid {
            return if entry.th32ParentProcessID == 0 {
                Err(denied())
            } else {
                Ok(entry.th32ParentProcessID)
            };
        }
        found = unsafe { Process32NextW(snapshot.as_raw_handle() as HANDLE, &mut entry) };
    }
    Err(denied())
}

fn ensure_not_inherited(handle: HANDLE) -> io::Result<()> {
    let mut flags = 0;
    if unsafe { GetHandleInformation(handle, &mut flags) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if flags & HANDLE_FLAG_INHERIT != 0 {
        Err(denied())
    } else {
        Ok(())
    }
}

struct LogonIdentity {
    owner: String,
    logon: String,
}

impl LogonIdentity {
    fn current() -> io::Result<Self> {
        let mut token = ptr::null_mut();
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let token = owned_handle(token)?;
        let user = token_information(token.as_raw_handle() as HANDLE, TokenUser)?;
        let owner = sid_string(unsafe { (*(user.as_ptr().cast::<TOKEN_USER>())).User.Sid })?;
        let groups = token_information(token.as_raw_handle() as HANDLE, TokenGroups)?;
        let groups_ptr = groups.as_ptr().cast::<TOKEN_GROUPS>();
        let count = unsafe { (*groups_ptr).GroupCount } as usize;
        let first = unsafe { (*groups_ptr).Groups.as_ptr() };
        let available = size_of_val(groups.as_slice())
            .checked_sub(first as usize - groups.as_ptr() as usize)
            .ok_or_else(denied)?;
        if count > available / size_of::<windows_sys::Win32::Security::SID_AND_ATTRIBUTES>() {
            return Err(denied());
        }
        // SAFETY: GetTokenInformation initialized an aligned buffer, with the
        // variable array bounds checked against that buffer above.
        let groups = unsafe { std::slice::from_raw_parts(first, count) };
        let logon = groups
            .iter()
            .find(|group| group.Attributes & SE_GROUP_LOGON_ID as u32 == SE_GROUP_LOGON_ID as u32)
            .ok_or_else(denied)?;
        Ok(Self {
            owner,
            logon: sid_string(logon.Sid)?,
        })
    }

    fn descriptor(&self) -> io::Result<LocalAllocation> {
        let sddl = format!(
            "O:{}D:P(A;;0x{PIPE_LOGON_ACCESS:08x};;;{})",
            self.owner, self.logon
        );
        descriptor_from_sddl(&sddl)
    }
}

fn token_information(token: HANDLE, class: TOKEN_INFORMATION_CLASS) -> io::Result<Vec<usize>> {
    let mut length = 0;
    unsafe { GetTokenInformation(token, class, ptr::null_mut(), 0, &mut length) };
    if length == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut data = vec![0usize; (length as usize).div_ceil(size_of::<usize>())];
    if unsafe { GetTokenInformation(token, class, data.as_mut_ptr().cast(), length, &mut length) }
        == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(data)
}

fn sid_string(sid: PSID) -> io::Result<String> {
    let mut string = ptr::null_mut();
    if unsafe { ConvertSidToStringSidW(sid, &mut string) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let allocation = LocalAllocation(string.cast());
    let mut length = 0;
    while length < 256 && unsafe { *string.add(length) } != 0 {
        length += 1;
    }
    if length == 256 {
        return Err(denied());
    }
    let result = String::from_utf16(unsafe { std::slice::from_raw_parts(string, length) })
        .map_err(|_| denied());
    drop(allocation);
    result
}

fn descriptor_from_sddl(sddl: &str) -> io::Result<LocalAllocation> {
    let encoded: Vec<_> = sddl.encode_utf16().chain(Some(0)).collect();
    let mut descriptor = ptr::null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            encoded.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(LocalAllocation(descriptor))
}

fn validate_security(handle: HANDLE, identity: &LogonIdentity) -> io::Result<()> {
    let mut owner = ptr::null_mut();
    let mut dacl: *mut ACL = ptr::null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    let result = unsafe {
        GetSecurityInfo(
            handle,
            SE_KERNEL_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            ptr::null_mut(),
            &mut dacl,
            ptr::null_mut(),
            &mut descriptor,
        )
    };
    if result != 0 {
        return Err(io::Error::from_raw_os_error(result as i32));
    }
    let _allocation = LocalAllocation(descriptor);
    let mut control = 0;
    let mut revision = 0;
    if unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if owner.is_null()
        || dacl.is_null()
        || control & SE_DACL_PROTECTED == 0
        || sid_string(owner)? != identity.owner
        || unsafe { (*dacl).AceCount } != 1
    {
        return Err(denied());
    }
    let mut ace = ptr::null_mut();
    if unsafe { GetAce(dacl, 0, &mut ace) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: GetSecurityInfo/GetAce return validated kernel-owned descriptor
    // bytes. We inspect the header before interpreting an allow ACE's fields.
    let header = unsafe { &*ace.cast::<ACE_HEADER>() };
    if header.AceType != ACCESS_ALLOWED_ACE_TYPE as u8
        || header.AceFlags != 0
        || (header.AceSize as usize) < size_of::<ACCESS_ALLOWED_ACE>()
    {
        return Err(denied());
    }
    let ace = unsafe { &*ace.cast::<ACCESS_ALLOWED_ACE>() };
    if ace.Mask != PIPE_LOGON_ACCESS
        || sid_string((&ace.SidStart as *const u32).cast_mut().cast())? != identity.logon
    {
        return Err(denied());
    }
    Ok(())
}

fn owned_handle(handle: HANDLE) -> io::Result<OwnedHandle> {
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: callers pass a newly allocated Windows handle exactly once.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

struct LocalAllocation(*mut c_void);
impl Drop for LocalAllocation {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}

fn denied() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "IPC requires private local inherited named pipes",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn private_named_pipes_transfer_bytes_with_noninheritable_parent_handles() {
        let identity = LogonIdentity::current().unwrap();
        let descriptor = identity.descriptor().unwrap();
        for parent_writes in [true, false] {
            let (mut server, mut client) =
                connected_pipe(&identity, &descriptor, parent_writes).unwrap();
            server.connect().await.unwrap();
            let mut session = 0;
            assert_ne!(
                unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut session) },
                0
            );
            validate_client(
                client.as_raw_handle() as HANDLE,
                &identity,
                unsafe { GetCurrentProcessId() },
                session,
            )
            .unwrap();
            if parent_writes {
                server.write_all(b"fixture").await.unwrap();
                let mut bytes = [0; 7];
                client.read_exact(&mut bytes).unwrap();
                assert_eq!(&bytes, b"fixture");
            } else {
                client.write_all(b"fixture").unwrap();
                let mut bytes = [0; 7];
                server.read_exact(&mut bytes).await.unwrap();
                assert_eq!(&bytes, b"fixture");
            }
        }
    }

    #[tokio::test]
    async fn connected_instance_refuses_extra_clients_and_pipe_instances() {
        let identity = LogonIdentity::current().unwrap();
        let descriptor = identity.descriptor().unwrap();
        let name = random_pipe_name().unwrap();
        let server = create_server(&name, &descriptor, true).unwrap();
        let _client = open_client(&name, true).unwrap();
        server.connect().await.unwrap();
        assert!(open_client(&name, true).is_err());
        assert!(create_server(&name, &descriptor, true).is_err());
    }

    #[tokio::test]
    async fn broad_acl_and_wrong_server_identity_are_refused() {
        let identity = LogonIdentity::current().unwrap();
        let descriptor =
            descriptor_from_sddl(&format!("O:{}D:P(A;;GA;;;WD)", identity.owner)).unwrap();
        let name = random_pipe_name().unwrap();
        let server = create_server(&name, &descriptor, true).unwrap();
        let client = open_client(&name, true).unwrap();
        server.connect().await.unwrap();
        assert!(validate_security(client.as_raw_handle() as HANDLE, &identity).is_err());
        let descriptor = identity.descriptor().unwrap();
        let (server, client) = connected_pipe(&identity, &descriptor, true).unwrap();
        server.connect().await.unwrap();
        let mut session = 0;
        let pid = unsafe { GetCurrentProcessId() };
        assert_ne!(unsafe { ProcessIdToSessionId(pid, &mut session) }, 0);
        assert!(
            validate_client(
                client.as_raw_handle() as HANDLE,
                &identity,
                pid.wrapping_add(1),
                session
            )
            .is_err()
        );
        assert!(
            validate_client(
                client.as_raw_handle() as HANDLE,
                &identity,
                pid,
                session.wrapping_add(1)
            )
            .is_err()
        );
        assert!(validate_client(ptr::null_mut(), &identity, 0, 0).is_err());
    }

    #[tokio::test]
    async fn another_logon_cannot_open_the_pipe() {
        let identity = LogonIdentity::current().unwrap();
        let (prefix, last) = identity.logon.rsplit_once('-').unwrap();
        let other_logon = format!("{prefix}-{}", last.parse::<u32>().unwrap().wrapping_add(1));
        let descriptor = descriptor_from_sddl(&format!(
            "O:{}D:P(A;;0x{PIPE_LOGON_ACCESS:08x};;;{other_logon})",
            identity.owner
        ))
        .unwrap();
        let name = random_pipe_name().unwrap();
        let _server = create_server(&name, &descriptor, true).unwrap();
        let error = open_client(&name, true).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    }
}
