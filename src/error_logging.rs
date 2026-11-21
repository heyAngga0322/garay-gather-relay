use crate::metrics::MetricsRecorder;
use std::fmt;
use tracing::{debug, error, info, warn};

/// Log an error with structured fields and increment error counter
pub fn log_error(error: &impl fmt::Display, context: &str, fields: &[(&str, &dyn fmt::Display)]) {
    // Build the field list as a formatted string for the macro
    let fields_str = fields
        .iter()
        .map(|(k, v)| format!("{} = {}", k, v))
        .collect::<Vec<_>>()
        .join(", ");

    error!(
        context = %context,
        error = %error,
        error_category = "application",
        fields = %fields_str,
        "Application error"
    );
    MetricsRecorder::error_recorded("application", context);
}

/// Log an error with a simple message and context
pub fn log_error_simple(error: &impl fmt::Display, context: &str) {
    error!(error = %error, context = %context, error_category = "application", "Application error");
    MetricsRecorder::error_recorded("application", context);
}

/// Log a warning with structured fields
pub fn log_warn(message: &str, fields: &[(&str, &dyn fmt::Display)]) {
    let fields_str = fields
        .iter()
        .map(|(k, v)| format!("{} = {}", k, v))
        .collect::<Vec<_>>()
        .join(", ");

    warn!(
        message = %message,
        fields = %fields_str,
        warn_category = "application",
        "Warning"
    );
}

/// Log a debug message with structured fields
pub fn log_debug(message: &str, fields: &[(&str, &dyn fmt::Display)]) {
    let fields_str = fields
        .iter()
        .map(|(k, v)| format!("{} = {}", k, v))
        .collect::<Vec<_>>()
        .join(", ");

    debug!(
        message = %message,
        fields = %fields_str,
        debug_category = "application",
        "Debug"
    );
}

/// Log WebSocket connection issues
pub fn log_ws_error(peer_id: &str, room_id: &str, error: &impl fmt::Display, context: &str) {
    error!(
        peer_id = %peer_id,
        room_id = %room_id,
        error = %error,
        context = %context,
        error_category = "websocket",
        "WebSocket error"
    );
    MetricsRecorder::error_recorded("websocket", context);
}

/// Log WebSocket slow client (backpressure) warnings
pub fn log_ws_slow_client(peer_id: &str, room_id: &str, message_type: &str) {
    warn!(
        peer_id = %peer_id,
        room_id = %room_id,
        message_type = %message_type,
        warn_category = "websocket_backpressure",
        "Dropping message for slow client"
    );
    MetricsRecorder::ws_slow_client();
}

/// Log WebRTC signaling errors
pub fn log_webrtc_error(peer_id: &str, room_id: &str, error: &impl fmt::Display, context: &str) {
    error!(
        peer_id = %peer_id,
        room_id = %room_id,
        error = %error,
        context = %context,
        error_category = "webrtc",
        "WebRTC error"
    );
    MetricsRecorder::error_recorded("webrtc", context);
}

/// Log WebRTC negotiation errors
pub fn log_webrtc_negotiation_error(
    peer_id: &str,
    room_id: &str,
    error: &impl fmt::Display,
    phase: &str,
) {
    error!(
        peer_id = %peer_id,
        room_id = %room_id,
        error = %error,
        phase = %phase,
        error_category = "webrtc_negotiation",
        "WebRTC negotiation error"
    );
    MetricsRecorder::error_recorded("webrtc_negotiation", phase);
}

/// Log RTP relay errors
pub fn log_rtp_relay_error(
    source_peer: &str,
    destination_peer: &str,
    room_id: &str,
    error: &impl fmt::Display,
) {
    error!(
        source_peer = %source_peer,
        destination_peer = %destination_peer,
        room_id = %room_id,
        error = %error,
        error_category = "rtp_relay",
        "RTP relay error"
    );
    MetricsRecorder::error_recorded("rtp_relay", "write_error");
}

/// Log RTP relay debug info (codec detection, track setup)
pub fn log_rtp_relay_debug(message: &str, fields: &[(&str, &dyn fmt::Display)]) {
    let fields_str = fields
        .iter()
        .map(|(k, v)| format!("{} = {}", k, v))
        .collect::<Vec<_>>()
        .join(", ");

    debug!(
        message = %message,
        fields = %fields_str,
        debug_category = "rtp_relay",
        "RTP relay debug"
    );
}

/// Log room admission errors
pub fn log_room_admission_error(room_id: &str, error: &impl fmt::Display, reason: &str) {
    warn!(
        room_id = %room_id,
        error = %error,
        reason = %reason,
        error_category = "room_admission",
        "Room admission rejected"
    );
    MetricsRecorder::error_recorded("room_admission", reason);
}

/// Log ticket/authentication errors
pub fn log_ticket_error(room_id: &str, error: &impl fmt::Display, reason: &str) {
    warn!(
        room_id = %room_id,
        error = %error,
        reason = %reason,
        error_category = "authentication",
        "Ticket validation failed"
    );
    MetricsRecorder::error_recorded("authentication", reason);
}

/// Log origin rejection
pub fn log_origin_rejected(origin: &str, allowed: &[String]) {
    warn!(
        origin = %origin,
        allowed_origins = ?allowed,
        error_category = "origin_check",
        "Origin rejected"
    );
    MetricsRecorder::error_recorded("origin_check", "rejected");
}

/// Log configuration errors
pub fn log_config_error(error: &impl fmt::Display, context: &str) {
    error!(
        error = %error,
        context = %context,
        error_category = "configuration",
        "Configuration error"
    );
    MetricsRecorder::error_recorded("configuration", context);
}

/// Log peer lifecycle events
pub fn log_peer_joined(peer_id: &str, room_id: &str, display_name: &str, role: &str) {
    info!(
        peer_id = %peer_id,
        room_id = %room_id,
        display_name = %display_name,
        role = %role,
        event_category = "peer_lifecycle",
        "Peer joined room"
    );
    MetricsRecorder::peer_joined(room_id);
}

pub fn log_peer_left(peer_id: &str, room_id: &str, display_name: &str) {
    info!(
        peer_id = %peer_id,
        room_id = %room_id,
        display_name = %display_name,
        event_category = "peer_lifecycle",
        "Peer left room"
    );
    MetricsRecorder::peer_left(room_id);
}

/// Log room lifecycle events
pub fn log_room_created(room_id: &str, owner: &str) {
    info!(
        room_id = %room_id,
        owner = %owner,
        event_category = "room_lifecycle",
        "Room created"
    );
    MetricsRecorder::room_created();
}

pub fn log_room_deleted(room_id: &str) {
    info!(
        room_id = %room_id,
        event_category = "room_lifecycle",
        "Room deleted"
    );
    MetricsRecorder::room_deleted();
}

/// Log ICE candidate events
pub fn log_ice_candidate(peer_id: &str, room_id: &str, candidate_type: &str) {
    debug!(
        peer_id = %peer_id,
        room_id = %room_id,
        candidate_type = %candidate_type,
        event_category = "ice",
        "ICE candidate received"
    );
}

/// Log SDP offer/answer events
pub fn log_sdp_exchange(peer_id: &str, room_id: &str, sdp_type: &str) {
    debug!(
        peer_id = %peer_id,
        room_id = %room_id,
        sdp_type = %sdp_type,
        event_category = "sdp",
        "SDP exchange"
    );
}

/// Log participant state changes
pub fn log_participant_state_change(
    peer_id: &str,
    room_id: &str,
    display_name: &str,
    muted: bool,
    camera_on: bool,
    screen_share: bool,
) {
    debug!(
        peer_id = %peer_id,
        room_id = %room_id,
        display_name = %display_name,
        muted = muted,
        camera_on = camera_on,
        screen_share = screen_share,
        event_category = "participant_state",
        "Participant state changed"
    );
}

/// Log server startup/shutdown
pub fn log_server_starting(address: &str) {
    info!(
        address = %address,
        event_category = "server_lifecycle",
        "WebRTC signaling server starting"
    );
}

pub fn log_server_shutdown() {
    info!(
        event_category = "server_lifecycle",
        "Shutdown signal received, draining connections..."
    );
}

/// Log ticket issuance
pub fn log_ticket_issued(role: &str, room_id: &str) {
    info!(
        role = %role,
        room_id = %room_id,
        event_category = "ticket",
        "Ticket issued"
    );
    MetricsRecorder::ticket_issued(role);
}

/// Log capacity rejection
pub fn log_capacity_rejected(room_id: &str, limit: usize) {
    warn!(
        room_id = %room_id,
        limit = limit,
        event_category = "capacity",
        "Room at capacity, rejecting peer"
    );
    MetricsRecorder::capacity_rejected(room_id);
}
