//! The only durable user data is this bounded catalogue of public contacts.
//! Session bindings, verification approvals, keys and messages never enter it.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};
use veyra_daemon::{PeerSummary, wire::Peer};

const VERSION: u32 = 1;
const MAX_CONTACTS: usize = 4096;
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct Contact {
    id: String,
    label: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Catalogue {
    version: u32,
    contacts: Vec<Contact>,
}

pub struct ContactBook {
    path: PathBuf,
    contacts: BTreeMap<String, Contact>,
}

impl ContactBook {
    /// Missing storage is a new catalogue. Invalid storage must never be reset.
    pub fn open(path: PathBuf) -> io::Result<Self> {
        let contacts = load(&path)?;
        Ok(Self { path, contacts })
    }

    /// The coordinator supplies peers only after checking their signed bindings.
    /// Missing peers are retained: an ended connection does not delete a contact.
    pub fn sync(&mut self, peers: &[Peer]) -> io::Result<()> {
        if peers.is_empty() {
            // Atomic replacement permits a reader to see either complete version.
            // This also refreshes another instance's changes without creating files.
            self.contacts = load(&self.path)?;
            return Ok(());
        }
        self.update(|contacts| {
            for peer in peers {
                identity(&peer.id)?;
                contacts.entry(peer.id.clone()).or_insert_with(|| Contact {
                    id: peer.id.clone(),
                    label: format!("Peer {}", &peer.id[..8]),
                });
            }
            if contacts.len() > MAX_CONTACTS {
                return Err(invalid("contact catalogue capacity exceeded"));
            }
            Ok(())
        })
    }

    pub fn remove(&mut self, id: &str) -> io::Result<()> {
        identity(id)?;
        self.update(|contacts| {
            contacts.remove(id);
            Ok(())
        })
    }

    /// A catalogue entry never constitutes a live connection or approval.
    pub fn summaries(&self) -> Vec<PeerSummary> {
        self.contacts
            .values()
            .map(|contact| PeerSummary {
                id: contact.id.clone(),
                label: contact.label.clone(),
                fingerprint: veyra_identity::fingerprint(
                    &identity(&contact.id).expect("validated contact identity"),
                ),
                verified: false,
                route: "disconnected",
                verification_code: None,
            })
            .collect()
    }

    fn update(
        &mut self,
        change: impl FnOnce(&mut BTreeMap<String, Contact>) -> io::Result<()>,
    ) -> io::Result<()> {
        let parent = self
            .path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .ok_or_else(|| invalid("contact catalogue needs a parent directory"))?;
        fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
        }

        // The lock survives replacement of contacts.json and contains no data.
        // Reload under this lock so concurrent app instances cannot lose additions.
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lock = options.open(parent.join("contacts.lock"))?;
        lock.lock()?;
        let mut contacts = load(&self.path)?;
        let previous = contacts.clone();
        change(&mut contacts)?;
        if contacts != previous {
            save(&self.path, &contacts)?;
        }
        self.contacts = contacts;
        Ok(())
    }
}

fn invalid(reason: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, reason)
}

fn identity(id: &str) -> io::Result<[u8; 32]> {
    if id.len() != 64
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(invalid("invalid public contact identity"));
    }
    let mut public = [0; 32];
    for (index, byte) in public.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&id[index * 2..index * 2 + 2], 16)
            .map_err(|_| invalid("invalid public contact identity"))?;
    }
    Ok(public)
}

fn load(path: &Path) -> io::Result<BTreeMap<String, Contact>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(error) => return Err(error),
    };
    if !file.metadata()?.is_file() {
        return Err(invalid("contact catalogue is not a regular file"));
    }
    let mut data = Vec::new();
    file.take(MAX_FILE_BYTES + 1).read_to_end(&mut data)?;
    if data.len() as u64 > MAX_FILE_BYTES {
        return Err(invalid("contact catalogue exceeds size limit"));
    }
    let catalogue: Catalogue =
        serde_json::from_slice(&data).map_err(|_| invalid("invalid contact catalogue"))?;
    if catalogue.version != VERSION || catalogue.contacts.len() > MAX_CONTACTS {
        return Err(invalid("unsupported contact catalogue"));
    }
    let mut contacts = BTreeMap::new();
    for contact in catalogue.contacts {
        identity(&contact.id)?;
        if contact.label.len() > 80
            || contact.label.trim().is_empty()
            || contact.label.chars().any(char::is_control)
        {
            return Err(invalid("invalid contact label"));
        }
        if contacts.insert(contact.id.clone(), contact).is_some() {
            return Err(invalid("duplicate contact identity"));
        }
    }
    Ok(contacts)
}

fn save(path: &Path, contacts: &BTreeMap<String, Contact>) -> io::Result<()> {
    let data = serde_json::to_vec(&Catalogue {
        version: VERSION,
        contacts: contacts.values().cloned().collect(),
    })
    .map_err(|_| invalid("cannot encode contact catalogue"))?;
    if data.len() as u64 > MAX_FILE_BYTES {
        return Err(invalid("contact catalogue exceeds size limit"));
    }
    let nonce = veyra_identity::random_id()
        .map_err(|_| io::Error::other("contact temporary name unavailable"))?;
    let nonce: String = nonce.iter().map(|byte| format!("{byte:02x}")).collect();
    let temporary = path.with_file_name(format!("contacts-{nonce}.tmp"));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    let result = (|| {
        file.write_all(&data)?;
        file.sync_all()?;
        drop(file);
        replace(&temporary, path)?;
        #[cfg(unix)]
        File::open(path.parent().expect("contact directory"))?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(not(windows))]
fn replace(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(windows)]
fn replace(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    // Both NUL-terminated strings remain alive for the synchronous system call.
    if unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct Directory(PathBuf);
    impl Directory {
        fn new() -> Self {
            let nonce = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "veyra-contact-test-{}-{nonce}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn path(&self) -> PathBuf {
            self.0.join("contacts.json")
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn peer(byte: u8) -> Peer {
        Peer {
            id: format!("{byte:02x}").repeat(32),
            binding_frame: b"SYNTHETIC-SESSION-BINDING".to_vec(),
            fingerprint: "UNTRUSTED-FINGERPRINT".into(),
            verification_code: "SYNTHETIC-VERIFICATION-CODE".into(),
            verified: true,
            online: true,
            route: veyra_connectivity::Route::Direct,
            available_offers: 1,
        }
    }

    #[test]
    fn new_catalogue_does_not_write_until_a_contact_exists() {
        let directory = Directory::new();
        let mut book = ContactBook::open(directory.path()).unwrap();
        book.sync(&[]).unwrap();
        assert!(book.summaries().is_empty());
        assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 0);
    }

    #[test]
    fn contacts_survive_restart_as_unverified_offline_public_metadata() {
        let directory = Directory::new();
        let mut book = ContactBook::open(directory.path()).unwrap();
        book.sync(&[peer(1)]).unwrap();
        drop(book);

        let restored = ContactBook::open(directory.path()).unwrap();
        let contacts = restored.summaries();
        assert_eq!(contacts.len(), 1);
        let contact = &contacts[0];
        assert_eq!(contact.id, peer(1).id);
        assert_eq!(contact.label, "Peer 01010101");
        assert_eq!(contact.fingerprint, veyra_identity::fingerprint(&[1; 32]));
        assert!(!contact.verified);
        assert_eq!(contact.route, "disconnected");
        assert!(contact.verification_code.is_none());

        let stored: serde_json::Value =
            serde_json::from_slice(&fs::read(directory.path()).unwrap()).unwrap();
        assert_eq!(stored.as_object().unwrap().len(), 2);
        assert_eq!(stored["version"], VERSION);
        assert_eq!(stored["contacts"][0].as_object().unwrap().len(), 2);
        assert_eq!(stored["contacts"][0]["id"], contact.id);
        assert_eq!(stored["contacts"][0]["label"], contact.label);
    }

    #[test]
    fn merge_preserves_absent_contacts_and_concurrent_additions() {
        let directory = Directory::new();
        let mut first = ContactBook::open(directory.path()).unwrap();
        let mut second = ContactBook::open(directory.path()).unwrap();
        first.sync(&[peer(1)]).unwrap();
        second.sync(&[peer(2)]).unwrap();
        first.sync(&[]).unwrap();
        assert_eq!(first.summaries().len(), 2);
        assert_eq!(second.summaries().len(), 2);
        assert_eq!(
            ContactBook::open(directory.path())
                .unwrap()
                .summaries()
                .len(),
            2
        );
    }

    #[test]
    fn removing_a_contact_is_durable_and_does_not_remove_other_contacts() {
        let directory = Directory::new();
        let mut book = ContactBook::open(directory.path()).unwrap();
        book.sync(&[peer(1), peer(2)]).unwrap();
        book.remove(&peer(1).id).unwrap();
        let mut restored = ContactBook::open(directory.path()).unwrap();
        assert_eq!(restored.summaries().len(), 1);
        assert_eq!(restored.summaries()[0].id, peer(2).id);
        restored.remove(&peer(2).id).unwrap();
        assert!(
            ContactBook::open(directory.path())
                .unwrap()
                .summaries()
                .is_empty()
        );
    }

    #[test]
    fn invalid_or_future_catalogues_are_rejected_without_overwriting() {
        let directory = Directory::new();
        for bytes in [
            b"{".to_vec(),
            br#"{"version":2,"contacts":[]}"#.to_vec(),
            br#"{"version":1,"contacts":[],"messages":[]}"#.to_vec(),
            serde_json::to_vec(&serde_json::json!({"version":1,"contacts":[{"id":peer(1).id,"label":"a","verified":true}]})).unwrap(),
            serde_json::to_vec(&serde_json::json!({"version":1,"contacts":[{"id":"bad","label":"a"}]})).unwrap(),
            serde_json::to_vec(&serde_json::json!({"version":1,"contacts":[{"id":peer(1).id,"label":"a"},{"id":peer(1).id,"label":"b"}]})).unwrap(),
            vec![b' '; MAX_FILE_BYTES as usize + 1],
        ] {
            fs::write(directory.path(), &bytes).unwrap();
            assert!(ContactBook::open(directory.path()).is_err());
            assert_eq!(fs::read(directory.path()).unwrap(), bytes);
        }
    }

    #[test]
    fn failed_write_does_not_change_in_memory_catalogue_or_existing_file() {
        let directory = Directory::new();
        let mut book = ContactBook::open(directory.path()).unwrap();
        book.sync(&[peer(1)]).unwrap();
        let previous = fs::read(directory.path()).unwrap();
        // A corrupt concurrent write is a deterministic storage error on every OS.
        fs::write(directory.path(), b"corrupt").unwrap();
        assert!(book.sync(&[peer(2)]).is_err());
        assert_eq!(book.summaries().len(), 1);
        assert_eq!(book.summaries()[0].id, peer(1).id);
        assert_eq!(fs::read(directory.path()).unwrap(), b"corrupt");
        fs::write(directory.path(), previous).unwrap();
        book.sync(&[peer(2)]).unwrap();
        assert_eq!(book.summaries().len(), 2);
        assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 2);
    }

    #[test]
    fn failed_atomic_replacement_preserves_destination_and_cleans_temporary_file() {
        let directory = Directory::new();
        fs::create_dir(directory.path()).unwrap();
        let sentinel = directory.path().join("existing-data");
        fs::write(&sentinel, b"preserve").unwrap();
        let contacts = BTreeMap::from([(
            peer(1).id.clone(),
            Contact {
                id: peer(1).id,
                label: "Synthetic contact".into(),
            },
        )]);
        assert!(save(&directory.path(), &contacts).is_err());
        assert_eq!(fs::read(sentinel).unwrap(), b"preserve");
        assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn catalogue_and_directory_are_private_to_the_user() {
        use std::os::unix::fs::PermissionsExt;
        let directory = Directory::new();
        let mut book = ContactBook::open(directory.path()).unwrap();
        book.sync(&[peer(1)]).unwrap();
        assert_eq!(
            fs::metadata(&directory.0).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(directory.path()).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
