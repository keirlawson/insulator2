use async_trait::async_trait;
use log::{debug, error, warn};
use std::{collections::HashMap, time::Duration, vec};

use super::{
    types::{PartitionInfo, Topic, TopicInfo},
    ConsumerGroupInfo,
};
use crate::lib::{
    admin::TopicPartitionOffset,
    configuration::{build_kafka_client_config, ClusterConfig},
    error::{Error, Result},
};
use rdkafka::{
    admin::{AdminClient, ResourceSpecifier},
    Offset, TopicPartitionList,
};
use rdkafka::{
    admin::{AdminOptions, NewTopic, TopicReplication},
    client::DefaultClientContext,
    consumer::{Consumer, StreamConsumer},
};

#[async_trait]
pub trait Admin {
    fn list_topics(&self) -> Result<Vec<Topic>>;
    async fn get_topic_info(&self, topic_name: &str) -> Result<TopicInfo>;
    async fn create_topic(&self, topic_name: &str, partitions: i32, isr: i32, compacted: bool) -> Result<()>;
    fn list_consumer_groups(&self) -> Result<Vec<String>>;
    fn describe_consumer_group(&self, consumer_group_name: &str) -> Result<ConsumerGroupInfo>;
}

pub struct KafkaAdmin {
    config: ClusterConfig,
    timeout: Duration,
    consumer: StreamConsumer,
    admin_client: AdminClient<DefaultClientContext>,
}

impl KafkaAdmin {
    pub fn new(config: &ClusterConfig) -> Self {
        KafkaAdmin {
            config: config.clone(),
            timeout: Duration::from_secs(30),
            consumer: build_kafka_client_config(config, None)
                .create()
                .expect("Unable to create a consumer for the admin client."),
            admin_client: build_kafka_client_config(config, None)
                .create()
                .expect("Unable to build the admin client"),
        }
    }
}

#[async_trait]
impl Admin for KafkaAdmin {
    fn list_topics(&self) -> Result<Vec<Topic>> {
        self.internal_list_topics(None)
    }

    async fn get_topic_info(&self, topic_name: &str) -> Result<TopicInfo> {
        let partitions_info = self.internal_list_topics(Some(topic_name))?;
        let responses = self
            .admin_client
            .describe_configs([&ResourceSpecifier::Topic(topic_name)], &AdminOptions::default())
            .await?;
        let mut configurations = HashMap::<String, Option<String>>::new();
        if let Some(Ok(topic_config)) = responses.first() {
            topic_config.entries.iter().for_each(|c| {
                configurations.insert(c.name.clone(), c.value.as_ref().cloned());
            })
        }
        debug!("Topic {} configurations {:?}", topic_name, configurations);
        if partitions_info.len() == 1 {
            let Topic { name, partitions } = partitions_info.get(0).unwrap();
            Ok(TopicInfo {
                name: name.to_string(),
                partitions: partitions.to_vec(),
                configurations,
            })
        } else {
            warn!(
                "Topic not found or more than one topic with the same name {}",
                topic_name
            );
            Err(Error::Kafka {
                message: "Topic not found".into(),
            })
        }
    }

    async fn create_topic(&self, name: &str, num_partitions: i32, isr: i32, compacted: bool) -> Result<()> {
        let new_topic = NewTopic {
            name,
            num_partitions,
            config: vec![("cleanup.policy", if compacted { "compact" } else { "delete" })],
            replication: TopicReplication::Fixed(isr),
        };
        let res = self
            .admin_client
            .create_topics(vec![&new_topic], &AdminOptions::default())
            .await?;
        let res = res.get(0).expect("Create topic: missing result");
        match res {
            Ok(_) => {
                debug!("Topic created successfully");
                Ok(())
            }
            Err(err) => {
                warn!("{:?}", err);
                Err(Error::Kafka {
                    message: format!("Unable to create the topic. {} {}", err.0, err.1),
                })
            }
        }
    }

    fn list_consumer_groups(&self) -> Result<Vec<String>> {
        let groups = self.consumer.fetch_group_list(None, self.timeout)?;
        let group_names: Vec<_> = groups.groups().iter().map(|g| g.name().to_string()).collect();
        Ok(group_names)
    }

    fn describe_consumer_group(&self, consumer_group_name: &str) -> Result<ConsumerGroupInfo> {
        // create a consumer with the defined consumer_group_name.
        // NOTE: the consumer shouldn't join the consumer group, otherwise it'll cause a re-balance
        debug!("Build the consumer for the consumer group {}", consumer_group_name);
        let consumer: StreamConsumer = build_kafka_client_config(&self.config, Some(consumer_group_name))
            .create()
            .expect("Unable to build the consumer");

        debug!("Build the list of all partitions and topics");
        let topics = self.list_topics()?;
        let mut topic_partition_lst = TopicPartitionList::new();
        topics.iter().for_each(|topic| {
            topic.partitions.iter().for_each(|partition| {
                topic_partition_lst.add_partition(&topic.name, partition.id);
            })
        });

        debug!("Check any committed offset to the consumer group");
        // allow up to 1 minute of tmo for big clusters and slow connections
        let res = consumer
            .committed_offsets(topic_partition_lst, Duration::from_secs(60))
            .unwrap();
        let offsets: Vec<_> = res
            .elements()
            .iter()
            .filter(|tpo| tpo.offset() != Offset::Invalid)
            .map(|r| TopicPartitionOffset {
                topic: r.topic().into(),
                partition_id: r.partition(),
                offset: map_offset(&r.offset()),
            })
            .collect();
        debug!("Retrieve completed {:?}", &offsets);

        Ok(ConsumerGroupInfo {
            name: consumer_group_name.into(),
            state: self.get_consumer_group_state(consumer_group_name)?,
            offsets,
        })
    }
}

impl KafkaAdmin {
    fn get_consumer_group_state(&self, consumer_group_name: &str) -> Result<String> {
        debug!("Retrieve consumer group status");
        let fetch_group_response = self
            .consumer
            .fetch_group_list(Some(consumer_group_name), self.timeout)?;
        let groups: Vec<_> = fetch_group_response.groups().iter().collect();
        assert_eq!(groups.len(), 1);
        Ok(groups[0].state().to_string())
    }

    fn internal_list_topics(&self, topic: Option<&str>) -> Result<Vec<Topic>> {
        let topics: Vec<Topic> = self
            .consumer
            .fetch_metadata(topic, self.timeout)?
            .topics()
            .iter()
            .map(|t| Topic {
                name: t.name().to_string(),
                partitions: t
                    .partitions()
                    .iter()
                    .map(|m| PartitionInfo {
                        id: m.id(),
                        isr: m.isr().len(),
                        replicas: m.replicas().len(),
                    })
                    .collect(),
            })
            .collect();
        Ok(topics)
    }
}

fn map_offset(offset: &Offset) -> i64 {
    match offset {
        Offset::Beginning => 0,
        Offset::Offset(v) => *v,
        _ => {
            error!("Unexpected offset {:?}. Returning -1.", offset);
            -1
        }
    }
}
