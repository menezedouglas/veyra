#![no_main]
use libfuzzer_sys::fuzz_target;
use veyra_protocol::{Frame,MAX_FRAME};
fuzz_target!(|bytes:&[u8]| {
    if let Ok(frame)=Frame::decode(bytes) {
        assert!(bytes.len()<=MAX_FRAME);
        assert_eq!(frame.encode(),bytes);
    }
});
