//! Outcome accounting for notification deliveries.

use crate::{
    names::{LABEL_LIMIT, LABEL_OUTCOME, NOTIFICATION_DELIVERIES_TOTAL, NOTIFICATION_DELIVERY_DURATION_SECONDS, NOTIFICATION_OVERSIZED_TOTAL},
    operation::Outcome,
};
use metrics::{counter, histogram};
use std::time::Instant;
use strum::IntoStaticStr;

/// The size limit a notification was refused under.
///
/// Rendered as the kebab-case `limit` label value. It names where the limit
/// came from, so an operator reads off the series which setting to change.
#[derive(Clone, Copy, Debug, Eq, PartialEq, IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub enum NotificationSizeLimit {
    /// `notifications.delivery.max_http_body`, the largest HTTP notification body.
    MaxHttpBody,
    /// `notifications.delivery.max_mqtt_packet`, the largest MQTT packet, where
    /// the MQTT server announced no smaller maximum.
    MaxMqttPacket,
    /// The maximum packet size an MQTT 5 server announced on connecting, below
    /// `notifications.delivery.max_mqtt_packet`.
    MqttServerAnnounced,
}

/// Count one notification refused for its size under `limit`.
pub fn count_oversized_notification(limit: NotificationSizeLimit) {
    counter!(NOTIFICATION_OVERSIZED_TOTAL, LABEL_LIMIT => <&'static str>::from(limit)).increment(1);
}

/// A running measurement of one notification delivery.
///
/// Records nothing until finished, so a timer dropped on an early return leaves
/// no misleading sample.
#[derive(Debug)]
pub struct DeliveryTimer {
    /// When the delivery attempt began.
    started: Instant,
}

impl DeliveryTimer {
    /// Start timing a delivery attempt.
    #[must_use]
    pub fn start() -> DeliveryTimer {
        DeliveryTimer { started: Instant::now() }
    }

    /// Record the elapsed time and count the delivery under `outcome`.
    pub fn finish(self, outcome: Outcome) {
        histogram!(NOTIFICATION_DELIVERY_DURATION_SECONDS).record(self.started.elapsed().as_secs_f64());
        counter!(NOTIFICATION_DELIVERIES_TOTAL, LABEL_OUTCOME => <&'static str>::from(outcome)).increment(1);
    }

    /// Finish with the outcome a [`Result`] represents.
    pub fn finish_result<T, E>(self, result: &Result<T, E>) {
        self.finish(Outcome::of(result));
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        delivery_ops::{DeliveryTimer, NotificationSizeLimit, count_oversized_notification},
        error::TelemetryError,
        names::{NOTIFICATION_DELIVERIES_TOTAL, NOTIFICATION_OVERSIZED_TOTAL},
        operation::Outcome,
        recorder::TelemetryRecorder,
    };

    #[test]
    fn a_delivery_is_counted_under_its_outcome() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached()?;
        recorder.record_locally(|| {
            DeliveryTimer::start().finish(Outcome::Ok);
            DeliveryTimer::start().finish(Outcome::Error);
        });
        let rendered = recorder.render();
        assert!(rendered.contains(&format!("{NOTIFICATION_DELIVERIES_TOTAL}{{outcome=\"ok\"}} 1")), "{rendered}");
        assert!(
            rendered.contains(&format!("{NOTIFICATION_DELIVERIES_TOTAL}{{outcome=\"error\"}} 1")),
            "{rendered}"
        );
        Ok(())
    }

    #[test]
    fn an_oversized_notification_is_counted_under_the_limit_that_refused_it() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached()?;
        recorder.record_locally(|| {
            count_oversized_notification(NotificationSizeLimit::MqttServerAnnounced);
            count_oversized_notification(NotificationSizeLimit::MqttServerAnnounced);
            count_oversized_notification(NotificationSizeLimit::MaxHttpBody);
        });
        let rendered = recorder.render();
        for (limit, count) in [("mqtt-server-announced", 2), ("max-http-body", 1)] {
            let sample = format!("{NOTIFICATION_OVERSIZED_TOTAL}{{limit=\"{limit}\"}} {count}");
            assert!(rendered.contains(&sample), "{rendered}");
        }
        assert!(!rendered.contains("max-mqtt-packet"), "{rendered}");
        Ok(())
    }
}
