use std::{collections::HashMap, sync::Arc};

use futures::lock::Mutex;
use log::debug;

use crate::lib::{
    admin::{Admin, KafkaAdmin},
    configuration::ClusterConfig,
    consumer::{Consumer, KafkaConsumer},
    parser::{Parser, RecordParser},
    record_store::TopicStore,
    schema_registry::{CachedSchemaRegistry, SchemaRegistryClient},
};

use super::record_store::AppStore;

type TopicName = String;

pub struct Cluster<SR = CachedSchemaRegistry, C = KafkaConsumer, P = RecordParser, A = KafkaAdmin>
where
    SR: SchemaRegistryClient + Send + Sync,
    C: Consumer + Send + Sync,
    P: Parser + Send + Sync,
    A: Admin + Send + Sync,
{
    pub config: ClusterConfig,
    pub schema_registry_client: Option<Arc<SR>>,
    pub admin_client: Arc<A>,
    pub parser: Arc<P>,
    pub app_store: Arc<AppStore>,
    consumers: Arc<Mutex<HashMap<TopicName, Arc<C>>>>,
}

impl Clone for Cluster<CachedSchemaRegistry> {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            schema_registry_client: self.schema_registry_client.clone(),
            consumers: self.consumers.clone(),
            admin_client: self.admin_client.clone(),
            parser: self.parser.clone(),
            app_store: self.app_store.clone(),
        }
    }
}

impl Cluster {
    pub fn new(config: &ClusterConfig) -> Self {
        let (schema_registry_client, parser) = {
            if let Some(s_config) = &config.schema_registry {
                let ptr = Arc::new(CachedSchemaRegistry::new(
                    s_config.endpoint.as_str(),
                    s_config.username.as_deref(),
                    s_config.password.as_deref(),
                ));
                (Some(ptr.clone()), RecordParser::new(Some(ptr)))
            } else {
                (None, RecordParser::new(None))
            }
        };
        Cluster {
            config: config.clone(),
            schema_registry_client,
            consumers: Arc::new(Mutex::new(HashMap::new())),
            admin_client: Arc::new(KafkaAdmin::new(config)),
            parser: Arc::new(parser),
            app_store: Arc::new(AppStore::new()),
        }
    }

    pub async fn get_consumer(&self, topic_name: &str) -> Arc<KafkaConsumer> {
        let mut consumers = self.consumers.lock().await;
        if consumers.get(topic_name).is_none() {
            debug!("Create consumer for topic {}", topic_name);
            // create a new table for the consumer
            let topic_store =
                TopicStore::from_app_store(self.app_store.clone(), self.parser.clone(), &self.config.id, topic_name)
                    .await;
            consumers.insert(
                topic_name.to_string(),
                Arc::new(KafkaConsumer::new(&self.config, topic_name, topic_store)),
            );
        }
        consumers.get(topic_name).expect("the consumer must exists").clone()
    }
}
