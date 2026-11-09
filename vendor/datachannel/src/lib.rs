#[cfg(not(any(feature = "log", feature = "tracing")))]
compile_error!("one of the features ['log', 'tracing'] must be enabled");

#[cfg(all(feature = "log", feature = "tracing"))]
compile_error!("only one of ['log', 'tracing'] can be enabled");

use std::sync::Once;

mod config;
mod datachannel;
mod error;
mod logger;
mod peerconnection;
mod track;

static INIT_LOGGING: Once = Once::new();

mod sys {
    use std::ffi::CStr;
    use std::os::raw::c_char;

    use datachannel_sys as sys;

    use crate::logger;

    pub(crate) unsafe extern "C" fn log_callback(level: sys::rtcLogLevel, message: *const c_char) {
        let message = CStr::from_ptr(message).to_string_lossy();
        match level {
            sys::rtcLogLevel_RTC_LOG_NONE => (),
            sys::rtcLogLevel_RTC_LOG_ERROR | sys::rtcLogLevel_RTC_LOG_FATAL => {
                logger::error!("{}", message)
            }
            sys::rtcLogLevel_RTC_LOG_WARNING => logger::warn!("{}", message),
            sys::rtcLogLevel_RTC_LOG_INFO => logger::info!("{}", message),
            sys::rtcLogLevel_RTC_LOG_DEBUG => logger::debug!("{}", message),
            sys::rtcLogLevel_RTC_LOG_VERBOSE => logger::trace!("{}", message),
            _ => unreachable!(),
        }
    }
}

#[cfg(feature = "log")]
fn ensure_logging() {
    INIT_LOGGING.call_once(|| {
        let level = match log::max_level() {
            log::LevelFilter::Off => datachannel_sys::rtcLogLevel_RTC_LOG_NONE,
            log::LevelFilter::Error => datachannel_sys::rtcLogLevel_RTC_LOG_ERROR,
            log::LevelFilter::Warn => datachannel_sys::rtcLogLevel_RTC_LOG_WARNING,
            log::LevelFilter::Info => datachannel_sys::rtcLogLevel_RTC_LOG_INFO,
            log::LevelFilter::Debug => datachannel_sys::rtcLogLevel_RTC_LOG_DEBUG,
            log::LevelFilter::Trace => datachannel_sys::rtcLogLevel_RTC_LOG_VERBOSE,
        };
        unsafe { datachannel_sys::rtcInitLogger(level, Some(sys::log_callback)) };
    });
}

fn ffi_string(ffi: &[u8]) -> crate::error::Result<String> {
    use std::ffi::CStr;
    let bytes = CStr::to_bytes(CStr::from_bytes_until_nul(ffi)?);
    Ok(String::from_utf8(bytes.to_vec())?)
}

#[cfg(test)]
mod ffi_string_tests {
    use super::ffi_string;

    #[test]
    fn selected_candidate_pair_accepts_unequal_length_buffers() {
        let host = "a=candidate:1 1 UDP 1 192.0.2.1 5000 typ host";
        let relay = "a=candidate:23456789 1 UDP 1 198.51.100.22 50000 typ relay raddr 192.0.2.2 rport 6000";
        assert_ne!(host.len(), relay.len());

        for (local, remote) in [(host, relay), (relay, host)] {
            // rtcGetSelectedCandidatePair reports the larger size, including NUL.
            // Both output buffers have this size; each string ends at its own NUL.
            let capacity = local.len().max(remote.len()) + 1;
            let mut local_buf = vec![0; capacity];
            let mut remote_buf = vec![0; capacity];
            local_buf[..local.len()].copy_from_slice(local.as_bytes());
            remote_buf[..remote.len()].copy_from_slice(remote.as_bytes());

            assert_eq!(ffi_string(&local_buf).unwrap(), local);
            assert_eq!(ffi_string(&remote_buf).unwrap(), remote);
        }
    }

    #[test]
    fn rejects_buffer_without_nul_terminator() {
        assert!(ffi_string(b"unterminated").is_err());
        assert!(ffi_string(b"").is_err());
    }

    #[test]
    fn rejects_invalid_utf8_before_terminator() {
        assert!(ffi_string(&[0xff, 0, 0]).is_err());
    }
}

/// An optional function to enable libdatachannel logging via `tracing`, otherwise it will be disabled.
#[cfg(feature = "tracing")]
pub fn configure_logging(level: tracing::Level) {
    INIT_LOGGING.call_once(|| {
        let level = match level {
            tracing::Level::ERROR => datachannel_sys::rtcLogLevel_RTC_LOG_ERROR,
            tracing::Level::WARN => datachannel_sys::rtcLogLevel_RTC_LOG_WARNING,
            tracing::Level::INFO => datachannel_sys::rtcLogLevel_RTC_LOG_INFO,
            tracing::Level::DEBUG => datachannel_sys::rtcLogLevel_RTC_LOG_DEBUG,
            tracing::Level::TRACE => datachannel_sys::rtcLogLevel_RTC_LOG_VERBOSE,
        };

        unsafe { datachannel_sys::rtcInitLogger(level, Some(sys::log_callback)) };
    });
}

/// An optional function to preload resources, otherwise they will be loaded lazily.
pub fn preload() {
    unsafe { datachannel_sys::rtcPreload() };
}

/// An optional resources cleanup function.
pub fn cleanup() {
    unsafe { datachannel_sys::rtcCleanup() };
}

pub use crate::config::{CertificateType, RtcConfig, TransportPolicy};
pub use crate::datachannel::{
    DataChannelHandler, DataChannelId, DataChannelInfo, DataChannelInit, Reliability,
    RtcDataChannel,
};
pub use crate::error::{Error, Result};
pub use crate::peerconnection::{
    fmt_sdp, serde_sdp, CandidatePair, ConnectionState, GatheringState, IceCandidate, IceState,
    PeerConnectionHandler, PeerConnectionId, RtcPeerConnection, SdpType, SessionDescription,
    SignalingState,
};
pub use crate::track::{Codec, Direction, RtcTrack, TrackHandler, TrackInit};

#[doc(inline)]
pub use webrtc_sdp as sdp;
