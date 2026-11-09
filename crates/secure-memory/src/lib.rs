//! Dedicated locked pages. Failure to lock is a failure to create the buffer.
//! This does not lock copies in other libraries, WebViews, drivers, or the OS.
use std::{io, ptr::NonNull, slice};
use zeroize::Zeroize;

pub struct LockedBuffer {
    ptr: NonNull<u8>,
    len: usize,
    allocation_len: usize,
}

// Ownership is exclusive; no aliases are manufactured by this type.
unsafe impl Send for LockedBuffer {}

impl LockedBuffer {
    pub fn new(len: usize) -> io::Result<Self> {
        if len == 0 || len > 16384 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid secure buffer size",
            ));
        }
        platform::allocate(len)
    }
    pub fn as_slice(&self) -> &[u8] {
        // SAFETY: allocation remains live, initialized, and uniquely owned.
        unsafe { slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
    }
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        // SAFETY: exclusive borrow ensures no concurrent references.
        unsafe { slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len) }
    }
}

impl Drop for LockedBuffer {
    fn drop(&mut self) {
        // Zeroize all page bytes while still locked, including unused tail.
        unsafe { slice::from_raw_parts_mut(self.ptr.as_ptr(), self.allocation_len) }.zeroize();
        platform::release(self);
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    pub fn allocate(len: usize) -> io::Result<LockedBuffer> {
        // sysconf result is validated before rounding.
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        if page <= 0 {
            return Err(io::Error::last_os_error());
        }
        let allocation_len = len.div_ceil(page as usize) * page as usize;
        let p = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                allocation_len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        if p == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }
        let fail = unsafe { libc::mlock(p, allocation_len) } != 0
            || unsafe { libc::madvise(p, allocation_len, libc::MADV_DONTDUMP) } != 0;
        if fail {
            let err = io::Error::last_os_error();
            unsafe {
                libc::munlock(p, allocation_len);
                libc::munmap(p, allocation_len);
            }
            return Err(err);
        }
        Ok(LockedBuffer {
            ptr: NonNull::new(p.cast()).expect("mmap non-null"),
            len,
            allocation_len,
        })
    }
    pub fn release(b: &mut LockedBuffer) {
        unsafe {
            libc::munlock(b.ptr.as_ptr().cast(), b.allocation_len);
            libc::munmap(b.ptr.as_ptr().cast(), b.allocation_len);
        }
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use windows_sys::Win32::System::ErrorReporting::{
        WerRegisterExcludedMemoryBlock, WerUnregisterExcludedMemoryBlock,
    };
    use windows_sys::Win32::System::Memory::*;
    pub fn allocate(len: usize) -> io::Result<LockedBuffer> {
        let allocation_len = len.div_ceil(4096) * 4096;
        let p = unsafe {
            VirtualAlloc(
                std::ptr::null(),
                allocation_len,
                MEM_COMMIT | MEM_RESERVE,
                PAGE_READWRITE,
            )
        };
        let Some(ptr) = NonNull::new(p.cast()) else {
            return Err(io::Error::last_os_error());
        };
        if unsafe { VirtualLock(p, allocation_len) } == 0 {
            let err = io::Error::last_os_error();
            unsafe {
                VirtualFree(p, 0, MEM_RELEASE);
            }
            return Err(err);
        }
        let result = unsafe { WerRegisterExcludedMemoryBlock(p, allocation_len as u32) };
        if result < 0 {
            unsafe {
                VirtualUnlock(p, allocation_len);
                VirtualFree(p, 0, MEM_RELEASE);
            }
            return Err(io::Error::from_raw_os_error(result));
        }
        Ok(LockedBuffer {
            ptr,
            len,
            allocation_len,
        })
    }
    pub fn release(b: &mut LockedBuffer) {
        unsafe {
            WerUnregisterExcludedMemoryBlock(b.ptr.as_ptr().cast());
            VirtualUnlock(b.ptr.as_ptr().cast(), b.allocation_len);
            VirtualFree(b.ptr.as_ptr().cast(), 0, MEM_RELEASE);
        }
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
mod platform {
    use super::*;
    pub fn allocate(_: usize) -> io::Result<LockedBuffer> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "unsupported platform",
        ))
    }
    pub fn release(_: &mut LockedBuffer) {}
}

/// Process-wide dump hardening, checked by callers before sensitive operations.
pub fn harden_process() -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        let limit = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        if unsafe { libc::setrlimit(libc::RLIMIT_CORE, &limit) } != 0
            || unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0) } != 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::ErrorReporting::{
            WER_FAULT_REPORTING_FLAG_NOHEAP, WerSetFlags,
        };
        let result = unsafe { WerSetFlags(WER_FAULT_REPORTING_FLAG_NOHEAP) };
        if result < 0 {
            return Err(io::Error::from_raw_os_error(result));
        }
        Ok(())
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "process dump hardening not validated",
    ))
}

/// Monotonic clock that includes Linux suspend time. A clock error refuses work.
pub fn monotonic_ms() -> io::Result<u64> {
    #[cfg(target_os = "linux")]
    {
        let mut time = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        if unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut time) } != 0 {
            return Err(io::Error::last_os_error());
        }
        if time.tv_sec < 0 || time.tv_nsec < 0 {
            return Err(io::Error::other("clock invalid"));
        }
        (time.tv_sec as u64)
            .checked_mul(1000)
            .and_then(|v| v.checked_add(time.tv_nsec as u64 / 1_000_000))
            .ok_or_else(|| io::Error::other("clock overflow"))
    }
    #[cfg(windows)]
    {
        Ok(unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() })
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "suspend-aware clock not validated",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn enforces_bounds() {
        assert!(LockedBuffer::new(0).is_err());
        assert!(LockedBuffer::new(16385).is_err());
    }
    #[test]
    fn actual_lock_or_explicit_failure() {
        match LockedBuffer::new(32) {
            Ok(mut b) => {
                assert_eq!(b.as_slice(), &[0; 32]);
                b.as_mut_slice().fill(7);
                assert_eq!(b.as_slice(), &[7; 32]);
            }
            Err(e) => assert!(
                e.raw_os_error().is_some(),
                "failure must come from checked native call"
            ),
        }
    }
}
