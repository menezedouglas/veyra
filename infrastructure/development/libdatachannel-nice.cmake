# Build datachannel-sys' EXISTING libdatachannel with maintained libnice ICE.
# libjuice does not support TURN TCP/TLS. Never silently fall back to libjuice.
set(USE_NICE ON CACHE BOOL "Veyra requires TURN TCP/TLS" FORCE)
set(NO_MEDIA ON CACHE BOOL "Veyra uses DataChannels only" FORCE)
set(NO_WEBSOCKET ON CACHE BOOL "Signaling belongs to Rust" FORCE)
set(NO_EXAMPLES ON CACHE BOOL "No sample endpoints" FORCE)
