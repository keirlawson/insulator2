use std::{collections::HashMap, io::Cursor, sync::Arc};

use apache_avro::{from_avro_datum, schema::Name, types::Value as AvroValue, Schema};
use num_bigint::BigInt;
use rust_decimal::Decimal;
use serde_json::{json, Map, Value as JsonValue};

use crate::lib::{
    error::{Error, Result},
    schema_registry::{CachedSchemaRegistry, SchemaRegistryClient},
};

pub struct AvroParser<C = CachedSchemaRegistry>
where
    C: SchemaRegistryClient + Send + Sync,
{
    schema_registry_client: Arc<C>,
}

impl<C> AvroParser<C>
where
    C: SchemaRegistryClient + Send + Sync,
{
    pub fn new(schema_registry_client: Arc<C>) -> Self {
        AvroParser { schema_registry_client }
    }

    pub async fn parse_payload(&self, raw: &[u8]) -> Result<String> {
        if raw.len() <= 5 || raw[0] != 0x00 {
            return Err(Error::AvroParse {
                message: "Supported avro messages should start with 0x00 follow by the schema id (4 bytes)".into(),
            });
        }

        let id = get_schema_id(raw)?;

        let schema = self
            .schema_registry_client
            .get_schema_by_id(id)
            .await
            .map_err(|err| Error::AvroParse {
                message: format!(
                    "{}\n{}",
                    "Unable to retrieve the schema from schema registry",
                    err.to_string()
                ),
            })?;
        let mut data = Cursor::new(&raw[5..]);
        let record = from_avro_datum(&schema, &mut data, None).map_err(|err| Error::AvroParse {
            message: format!("{}\n{}", "Unable to parse the avro record", err),
        })?;
        let json = map(&record, &schema, &mut HashMap::new())?;
        let res = serde_json::to_string(&json).map_err(|err| Error::AvroParse {
            message: format!("{}\n{}", "Unable to map the avro record to json", err),
        })?; // todo: maybe pretty_print
        Ok(res)
    }
}

fn get_schema_id(raw: &[u8]) -> Result<i32> {
    let arr = <[u8; 4]>::try_from(&raw[1..5]).map_err(|_| Error::AvroParse {
        message: "Invalid record. Unable to extract the schema id.".into(),
    })?;
    Ok(i32::from_be_bytes(arr))
}

fn map<'a>(
    value: &AvroValue,
    schema: &'a Schema,
    ref_cache: &mut HashMap<&'a Name, &'a Schema>, //cache to resolve avro references
) -> Result<JsonValue> {
    match (value, schema) {
        (AvroValue::Null, Schema::Null) => Ok(JsonValue::Null),
        (AvroValue::Boolean(v), Schema::Boolean) => Ok(json!(*v)),
        (AvroValue::Int(v), Schema::Int) => Ok(json!(*v)),
        (AvroValue::Long(v), Schema::Long) => Ok(json!(*v)),
        (AvroValue::Float(v), Schema::Float) => Ok(json!(*v)),
        (AvroValue::Double(v), Schema::Double) => Ok(json!(*v)),
        (AvroValue::String(v), Schema::String) => Ok(json!(*v)),
        (AvroValue::Array(v), Schema::Array(s)) => {
            let mut json_vec = Vec::new();
            for v in v.iter() {
                json_vec.push(map(v, s, ref_cache)?);
            }
            Ok(JsonValue::Array(json_vec))
        }
        (AvroValue::Map(vec), Schema::Map(s)) => {
            //todo: DRY
            let mut json_map = Map::new();
            for (k, v) in vec.iter() {
                json_map.insert(k.clone(), map(v, s, ref_cache)?);
            }
            Ok(JsonValue::Object(json_map))
        }
        (
            AvroValue::Record(vec),
            Schema::Record {
                name, fields, lookup, ..
            },
        ) => {
            ref_cache.insert(name, schema);
            let mut json_map = Map::new();
            for (k, v) in vec.iter() {
                let field_index = lookup.get(k).unwrap_or_else(|| panic!("Missing field {}", k));
                json_map.insert(k.clone(), map(v, &fields.get(*field_index).unwrap().schema, ref_cache)?);
            }
            Ok(JsonValue::Object(json_map))
        }
        (AvroValue::Date(v), Schema::Date) => Ok(json!(*v)),
        (AvroValue::TimeMillis(v), Schema::TimeMillis) => Ok(json!(*v)),
        (AvroValue::TimeMicros(v), Schema::TimeMicros) => Ok(json!(*v)),
        (AvroValue::TimestampMillis(v), Schema::TimestampMillis) => Ok(json!(*v)),
        (AvroValue::TimestampMicros(v), Schema::TimestampMicros) => Ok(json!(*v)),
        (AvroValue::Uuid(v), Schema::Uuid) => Ok(json!(*v)),
        //todo: WIP
        (AvroValue::Bytes(v), Schema::Bytes) => Ok(json!(*v)), //todo: this should be like "\u00FF"
        (
            AvroValue::Decimal(v),
            Schema::Decimal {
                precision: _,
                scale,
                inner: _,
            },
        ) => {
            let arr = <Vec<u8>>::try_from(v).expect("Invalid decimal received");
            let value = BigInt::from_signed_bytes_be(&arr);
            let decimal = Decimal::new(
                i64::try_from(value).expect("Unable to cast to i64"),
                scale.to_owned() as u32,
            );
            Ok(json!(decimal))
        }
        (AvroValue::Duration(v), Schema::Duration) => {
            //todo: check avro json representation
            Ok(json!(format!(
                "{:?} months {:?} days {:?} millis",
                v.months(),
                v.days(),
                v.millis()
            )))
        }
        //todo: use avro-json format
        (AvroValue::Union(i, v), Schema::Union(s)) => {
            let schema = s.variants().get(*i as usize).expect("Missing schema in the union");
            for s in s.variants() {
                if let Schema::Record { name, .. } = s {
                    ref_cache.insert(name, s);
                }
            }
            map(&**v, schema, ref_cache)
        }
        (AvroValue::Enum(_, v), Schema::Enum { name, .. }) => {
            ref_cache.insert(name, schema);
            Ok(json!(*v))
        }
        //todo: check representation in avro-json
        (AvroValue::Fixed(_, v), Schema::Fixed { name, .. }) => {
            ref_cache.insert(name, schema);
            Ok(json!(*v))
        }
        (value, Schema::Ref { name }) => {
            let schema = ref_cache
                .get(name)
                .unwrap_or_else(|| panic!("Missing Avro schema reference {:?}", name));
            map(value, schema, ref_cache)
        }
        (_, s) => panic!("Unexpected value/schema tuple. Schema: {:?}", s),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use apache_avro::{to_avro_datum, types::Record, types::Value as AvroValue, Schema as ApacheAvroSchema, Writer};
    use async_trait::async_trait;

    use crate::lib::schema_registry::{Result, SchemaRegistryClient, Subject};

    use super::{get_schema_id, AvroParser};
    struct MockSchemaRegistry {
        schema: String,
    }

    #[async_trait]
    impl SchemaRegistryClient for MockSchemaRegistry {
        async fn list_subjects(&self) -> Result<Vec<String>> {
            todo!()
        }
        async fn get_subject(&self, _: &str) -> Result<Subject> {
            todo!()
        }
        async fn get_schema_by_id(&self, _: i32) -> Result<ApacheAvroSchema> {
            Ok(ApacheAvroSchema::parse_str(&self.schema).unwrap())
        }
        async fn delete_subject(&self, _: &str) -> Result<()> {
            todo!()
        }

        async fn delete_version(&self, _: &str, _: i32) -> Result<()> {
            todo!()
        }
    }
    fn get_sut(schema: String) -> AvroParser<MockSchemaRegistry> {
        AvroParser::new(Arc::new(MockSchemaRegistry { schema }))
    }

    #[tokio::test]
    async fn test_simple_types_parsing() {
        let raw_schema = r#"
    {
        "fields": [
            { "name": "null_field", "type": "null" },
            { "name": "boolean_field", "type": "boolean" },
            { "name": "int_field", "type": "int" },
            { "name": "long_field", "type": "long" },
            { "name": "float_field", "type": "float" },
            { "name": "double_field", "type": "double" },
            { "name": "bytes_field", "type": "bytes" },
            { "name": "string_field", "type": "string" }
        ],
        "name": "sampleRecord",
        "namespace": "com.example.namespace",
        "type": "record"
      }
"#;
        let schema = ApacheAvroSchema::parse_str(raw_schema).unwrap();
        let writer = Writer::new(&schema, Vec::new());
        let mut record = Record::new(writer.schema()).unwrap();
        record.put("null_field", AvroValue::Null);
        record.put("boolean_field", true);
        record.put("int_field", 12);
        record.put("long_field", 12345667);
        record.put("float_field", 123.123f32);
        record.put("double_field", 12.12f64);
        record.put("bytes_field", AvroValue::Bytes(vec![0x01, 0x02, 0xaa]));
        record.put("string_field", "YO!! test");
        let mut encoded = to_avro_datum(&schema, record).unwrap();
        // add 1 magic byte + 4 id bytes
        let mut raw: Vec<u8> = vec![0x00, 0x00, 0x00, 0x00, 0x00];
        raw.append(&mut encoded);

        let res = get_sut(raw_schema.to_string()).parse_payload(&raw[..]).await.unwrap();

        assert_eq!(
            res,
            r#"{"boolean_field":true,"bytes_field":[1,2,170],"double_field":12.12,"float_field":123.12300109863281,"int_field":12,"long_field":12345667,"null_field":null,"string_field":"YO!! test"}"#
        )
    }

    #[test]
    fn u8_array_to_i32() {
        let raw: Vec<u8> = vec![0x00, 0x00, 0x01, 0x86, 0xc5, 0x00, 0x00, 0x00];
        let id = get_schema_id(&raw).unwrap();
        assert_eq!(id, 100037)
    }
}
