//! The delivery profile a ROS 2 endpoint runs under.
//!
//! A device carries two kinds of traffic over the same graph, and they want
//! opposite guarantees. Continuous state — a rig's joint values, the rendered
//! face — is only interesting at its newest value, and a reader that cannot
//! keep up must not be allowed to hold the writer back. A command — speech
//! text, an expression change — is an instruction, and dropping one loses it.
//! [`Qos`] names those two, and each backend maps it onto its own profile type
//! ([`rustdds`'s `QosPolicies`](ros2_client::ros2::QosPolicies) on `dds`,
//! [`QosProfile`](ros2_client::QosProfile) on `zenoh`).

use crate::profile::Flow;

/// How a ROS 2 endpoint delivers its samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Qos {
    /// Continuous state: **best-effort, volatile, keep-last-1** — ROS's
    /// `rmw_qos_profile_sensor_data`, at depth 1. The freshest sample is the
    /// only one that matters, so a slow or absent reader costs a dropped
    /// sample rather than a stalled writer.
    ///
    /// The trade is DDS's matching rule: a best-effort writer does not match a
    /// reliable reader. A subscriber left on the rclcpp/rclpy default
    /// (reliable) sees nothing on such a topic until it asks for best-effort —
    /// the same bargain `image_transport` strikes for camera streams.
    SensorData,
    /// Commands and one-shot values: **reliable, volatile, keep-last-1**. A
    /// lost sample is retransmitted, because losing it loses an instruction.
    Reliable,
}

impl Qos {
    /// What an endpoint gets when it declares nothing: state flowing out is
    /// sensor data, commands flowing in are reliable.
    pub fn default_for(flow: Flow) -> Self {
        match flow {
            Flow::Out => Qos::SensorData,
            Flow::In => Qos::Reliable,
        }
    }
}

/// How long a reliable writer may block waiting for a reader to acknowledge
/// before it gives up on a sample. Bounded so a wedged reader cannot stop the
/// publish loop indefinitely — the same figure the service plane uses.
#[cfg(feature = "dds")]
const MAX_BLOCKING_TIME_MS: i64 = 100;

/// The `rustdds` policies for a profile.
#[cfg(feature = "dds")]
pub(crate) fn dds(qos: Qos) -> ros2_client::ros2::QosPolicies {
    use ros2_client::ros2::{policy, Duration, QosPolicyBuilder};
    let reliability = match qos {
        Qos::SensorData => policy::Reliability::BestEffort,
        Qos::Reliable => policy::Reliability::Reliable {
            max_blocking_time: Duration::from_millis(MAX_BLOCKING_TIME_MS),
        },
    };
    QosPolicyBuilder::new()
        .reliability(reliability)
        .durability(policy::Durability::Volatile)
        .history(policy::History::KeepLast { depth: 1 })
        .build()
}

/// The backend-neutral profile for a profile, as the Zenoh backend takes it.
#[cfg(feature = "zenoh")]
pub(crate) fn zenoh(qos: Qos) -> ros2_client::QosProfile {
    use ros2_client::qos::{Durability, History, Reliability};
    let reliability = match qos {
        Qos::SensorData => Reliability::BestEffort,
        Qos::Reliable => Reliability::Reliable,
    };
    ros2_client::QosProfile::subscription_default()
        .reliability(reliability)
        .durability(Durability::Volatile)
        .history(History::KeepLast { depth: 1 })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The defaults are the point of the type: nothing else in the bridge has
    /// to know which way a given endpoint leans.
    #[test]
    fn state_flows_out_best_effort_and_commands_come_in_reliable() {
        assert_eq!(Qos::default_for(Flow::Out), Qos::SensorData);
        assert_eq!(Qos::default_for(Flow::In), Qos::Reliable);
    }

    #[cfg(feature = "dds")]
    #[test]
    fn both_profiles_keep_only_the_latest_sample() {
        use ros2_client::ros2::policy;
        for qos in [Qos::SensorData, Qos::Reliable] {
            assert_eq!(
                dds(qos).history(),
                Some(policy::History::KeepLast { depth: 1 })
            );
            assert_eq!(dds(qos).durability(), Some(policy::Durability::Volatile));
        }
        assert_eq!(
            dds(Qos::SensorData).reliability(),
            Some(policy::Reliability::BestEffort)
        );
        assert!(matches!(
            dds(Qos::Reliable).reliability(),
            Some(policy::Reliability::Reliable { .. })
        ));
    }
}
