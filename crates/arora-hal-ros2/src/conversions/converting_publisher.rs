//! Converting publisher for publishing StateChange to ROS2 topics.
//!
//! This module provides a trait and implementation for publishing state changes
//! to ROS2 topics with automatic conversion from StateChange to typed ROS2 messages.

use std::collections::HashMap;

use arora_types::data::StateChange;
use async_trait::async_trait;
use log::{debug, error};
use ros2_client::{Node, Publisher};

use crate::{
    config::TopicMapping,
    conversions::{self, FromStateChange},
    msgs::MessageType,
    ros2_error::ROS2RobotError,
};

/// A trait for publishers that can convert and publish state changes to ROS2 topics.
#[async_trait]
pub(crate) trait StateChangePublisher: Send + Sync {
    /// Publish a state change to the ROS2 topic.
    async fn publish_state_change(
        &self,
        state_change: &StateChange,
        topic_mapping: &TopicMapping,
        joint_ids_to_ros_names: &HashMap<String, String>,
    ) -> Result<(), ROS2RobotError>;

    /// Wait until at least one subscriber is connected to this publisher.
    /// Requires a spinner to be running on the node.
    fn wait_for_subscription(
        &self,
        node: &Node,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>>;
}

/// A typed publisher that converts StateChange to a specific message type before publishing.
pub(crate) struct ConvertingStateChangePublisher<
    T: MessageType + FromStateChange + serde::Serialize,
> {
    publisher: Publisher<T>,
    topic_name: String,
}

impl<T: MessageType + FromStateChange + serde::Serialize> ConvertingStateChangePublisher<T> {
    /// Create a new ConvertingStateChangePublisher.
    pub fn new(publisher: Publisher<T>, topic_name: String) -> Self {
        Self {
            publisher,
            topic_name,
        }
    }
}

#[async_trait]
impl<T: MessageType + FromStateChange + serde::Serialize> StateChangePublisher
    for ConvertingStateChangePublisher<T>
{
    async fn publish_state_change(
        &self,
        state_change: &StateChange,
        topic_mapping: &TopicMapping,
        joint_ids_to_ros_names: &HashMap<String, String>,
    ) -> Result<(), ROS2RobotError> {
        let message = conversions::state_change_to_message::<T>(
            state_change,
            topic_mapping,
            &self.topic_name,
            joint_ids_to_ros_names,
        )
        .map_err(|e| {
            error!(
                "Failed to convert state change to message for topic '{}': {}",
                self.topic_name, e
            );
            ROS2RobotError::ConversionError(e.to_string())
        })?;

        // If conversion returned Some(message), publish it
        if let Some(message) = message {
            self.publisher.async_publish(message).await.map_err(|e| {
                error!(
                    "Failed to publish message to topic '{}': {:?}",
                    self.topic_name, e
                );
                ROS2RobotError::PublisherError {
                    topic: self.topic_name.clone(),
                    reason: format!("{:?}", e),
                }
            })?;

            debug!(
                "Successfully published message to topic '{}'",
                self.topic_name
            );
        }
        Ok(())
    }

    fn wait_for_subscription(
        &self,
        node: &Node,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        Box::pin(self.publisher.wait_for_subscription(node))
    }
}
