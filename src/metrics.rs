use metrics::{counter, gauge, histogram};
use std::time::{Duration, Instant};

pub struct MetricsRecorder;

impl MetricsRecorder {
    pub fn room_created() {
        counter!("garay_rooms_created_total").increment(1);
    }

    pub fn room_deleted() {
        counter!("garay_rooms_deleted_total").increment(1);
    }

    pub fn peer_joined(room_id: &str) {
        counter!("garay_peers_joined_total", "room_id" => room_id.to_owned()).increment(1);
        gauge!("garay_peers_in_room", "room_id" => room_id.to_owned()).increment(1.0);
    }

    pub fn peer_left(room_id: &str) {
        counter!("garay_peers_left_total", "room_id" => room_id.to_owned()).increment(1);
        gauge!("garay_peers_in_room", "room_id" => room_id.to_owned()).decrement(1.0);
    }

    pub fn track_published(room_id: &str, kind: &str) {
        counter!("garay_tracks_published_total", "room_id" => room_id.to_owned(), "kind" => kind.to_owned()).increment(1);
        gauge!("garay_tracks_in_room", "room_id" => room_id.to_owned()).increment(1.0);
    }

    pub fn track_unpublished(room_id: &str) {
        counter!("garay_tracks_unpublished_total", "room_id" => room_id.to_owned()).increment(1);
        gauge!("garay_tracks_in_room", "room_id" => room_id.to_owned()).decrement(1.0);
    }

    pub fn relay_task_started() {
        counter!("garay_relay_tasks_started_total").increment(1);
        gauge!("garay_relay_tasks_active").increment(1.0);
    }

    pub fn relay_task_stopped() {
        counter!("garay_relay_tasks_stopped_total").increment(1);
        gauge!("garay_relay_tasks_active").decrement(1.0);
    }

    pub fn rtp_packet_forwarded(room_id: &str, kind: &str, bytes: usize) {
        counter!("garay_rtp_packets_forwarded_total", "room_id" => room_id.to_owned(), "kind" => kind.to_owned()).increment(1);
        histogram!("garay_rtp_packet_bytes", "room_id" => room_id.to_owned(), "kind" => kind.to_owned()).record(bytes as f64);
    }

    pub fn rtp_packet_dropped(room_id: &str, reason: &str) {
        counter!("garay_rtp_packets_dropped_total", "room_id" => room_id.to_owned(), "reason" => reason.to_owned()).increment(1);
    }

    pub fn signaling_message_received(kind: &str) {
        counter!("garay_signaling_messages_received_total", "kind" => kind.to_owned()).increment(1);
    }

    pub fn signaling_message_sent(kind: &str) {
        counter!("garay_signaling_messages_sent_total", "kind" => kind.to_owned()).increment(1);
    }

    pub fn webrtc_negotiation_duration(room_id: &str, duration: Duration) {
        histogram!("garay_webrtc_negotiation_seconds", "room_id" => room_id.to_owned())
            .record(duration.as_secs_f64());
    }

    pub fn peer_connection_established(room_id: &str, duration: Duration) {
        histogram!("garay_peer_connection_established_seconds", "room_id" => room_id.to_owned())
            .record(duration.as_secs_f64());
    }

    pub fn ticket_issued(role: &str) {
        counter!("garay_tickets_issued_total", "role" => role.to_owned()).increment(1);
    }

    pub fn ticket_rejected(reason: &str) {
        counter!("garay_tickets_rejected_total", "reason" => reason.to_owned()).increment(1);
    }

    pub fn origin_rejected() {
        counter!("garay_origins_rejected_total").increment(1);
    }

    pub fn capacity_rejected(room_id: &str) {
        counter!("garay_capacity_rejections_total", "room_id" => room_id.to_owned()).increment(1);
    }

    pub fn error_recorded(category: &str, context: &str) {
        counter!("garay_errors_total", "category" => category.to_owned(), "context" => context.to_owned()).increment(1);
    }

    pub fn ws_slow_client() {
        counter!("garay_ws_slow_client_total").increment(1);
    }
}

pub struct TimedScope {
    start: Instant,
    name: &'static str,
    labels: Vec<(&'static str, &'static str)>,
}

impl TimedScope {
    pub fn new(name: &'static str, labels: Vec<(&'static str, &'static str)>) -> Self {
        Self {
            start: Instant::now(),
            name,
            labels,
        }
    }
}

impl Drop for TimedScope {
    fn drop(&mut self) {
        let duration = self.start.elapsed();
        let labels_ref: Vec<(&str, &str)> = self.labels.iter().map(|(k, v)| (*k, *v)).collect();
        histogram!(self.name, &labels_ref).record(duration.as_secs_f64());
    }
}
