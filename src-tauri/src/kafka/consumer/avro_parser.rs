use std::io::Cursor;

use apache_avro::{ from_avro_datum, types::Value as AvroValue, Schema };
use rdkafka::{ message::OwnedMessage, Message };
use serde_json::{ Map, Value as JsonValue };

use crate::{
    configuration::SchemaRegistry,
    error::{ Result, TauriError },
    kafka::consumer::KafkaRecord,
    schema_registry::{ BasicAuth, CachedSchemaRegistry, ReqwestClient, SchemaRegistryClient },
};

use super::string_parser::parse_string;

pub(super) async fn parse_record(msg: OwnedMessage, config: &SchemaRegistry) -> Result<KafkaRecord> {
    let value = match msg.payload() {
        Some(x) => Some(parse_avro(x, config).await?),
        None => None,
    };

    Ok(KafkaRecord {
        key: parse_string(msg.key()), //todo: support avro key
        value,
        offset: msg.offset(),
        partition: msg.partition(),
        timestamp: match msg.timestamp() {
            rdkafka::Timestamp::NotAvailable => None,
            rdkafka::Timestamp::CreateTime(t) => Some(t),
            rdkafka::Timestamp::LogAppendTime(t) => Some(t),
        },
    })
}

pub async fn parse_avro(raw: &[u8], config: &SchemaRegistry) -> Result<String> {
    if raw.len() <= 5 || raw[0] != 0x00 {
        return Err(TauriError {
            error_type: "Invalid AVRO byte array received".into(),
            message: "Supported avro messages should start with 0x00 follow by the schema id (4 bytes)".into(),
        });
    }

    let id = get_schema_id(raw)?;

    //todo: inject the schema registry client
    let http_client = ReqwestClient::new(None);
    let client = CachedSchemaRegistry::new(
        config.endpoint.clone(),
        Some(BasicAuth {
            username: config.clone().username.unwrap(),
            password: Some(config.clone().password.unwrap()),
        }),
        http_client
    );
    let raw_schema = client.get_schema_by_id(id).await?;
    let schema = Schema::parse_str(raw_schema.as_str()).map_err(|err| TauriError {
        error_type: "Unable to parse the schema from schema registry".into(),
        message: err.to_string(),
    })?;
    let mut data = Cursor::new(&raw[5..]);
    let record = from_avro_datum(&schema, &mut data, None).map_err(|err| TauriError {
        error_type: "Unable to parse the avro record".into(),
        message: err.to_string(),
    })?;
    let json = map(&record)?;
    let res = serde_json::to_string(&json)?; // todo: maybe pretty_print
    Ok(res)
}

fn get_schema_id(raw: &[u8]) -> Result<i32> {
    let arr = <[u8; 4]>::try_from(&raw[1..5]).map_err(|_| TauriError {
        error_type: "Invalid schema received".into(),
        message: "".into(),
    })?;
    Ok(i32::from_be_bytes(arr))
}

fn map(a: &AvroValue) -> Result<JsonValue> {
    match a {
        AvroValue::Null => Ok(JsonValue::Null),
        AvroValue::Boolean(v) => Ok(JsonValue::Bool(*v)),
        AvroValue::Int(v) => Ok(JsonValue::Number(serde_json::Number::from(*v))),
        AvroValue::Long(v) => Ok(JsonValue::Number(serde_json::Number::from(*v))),
        AvroValue::Float(_v) => todo!(),
        AvroValue::Double(_v) => todo!(),
        AvroValue::Bytes(_) => todo!(),
        AvroValue::String(v) => Ok(JsonValue::String(v.clone())),
        AvroValue::Fixed(_, _) => todo!(),
        AvroValue::Enum(_, _) => todo!(),
        AvroValue::Union(_, v) => map(&**v),
        AvroValue::Array(_) => todo!(),
        AvroValue::Map(vec) => {
            //todo: DRY
            let mut json_map = Map::new();
            for (k, v) in vec.iter() {
                json_map.insert(k.clone(), map(v)?);
            }
            Ok(JsonValue::Object(json_map))
        }
        AvroValue::Record(vec) => {
            let mut json_map = Map::new();
            for (k, v) in vec.iter() {
                json_map.insert(k.clone(), map(v)?);
            }
            Ok(JsonValue::Object(json_map))
        }
        AvroValue::Date(_) => todo!(),
        AvroValue::Decimal(_) => todo!(),
        AvroValue::TimeMillis(_) => todo!(),
        AvroValue::TimeMicros(_) => todo!(),
        AvroValue::TimestampMillis(_) => todo!(),
        AvroValue::TimestampMicros(_) => todo!(),
        AvroValue::Duration(_) => todo!(),
        AvroValue::Uuid(_) => todo!(),
    }
}

#[cfg(test)]
mod tests {
    use apache_avro::{ to_avro_datum, types::Record, Schema, Writer };

    use super::{ get_schema_id, parse_avro };

    #[test]
    fn poc_avro() {
        let raw_schema =
            r#"
    {
        "doc": "Sample schema to help you get started.",
        "fields": [
          {
            "doc": "The int type is a 32-bit signed integer.",
            "name": "my_field1",
            "type": "int"
          }
        ],
        "name": "sampleRecord",
        "namespace": "com.mycorp.mynamespace",
        "type": "record"
      }
"#;
        let schema = Schema::parse_str(raw_schema).unwrap();
        let writer = Writer::new(&schema, Vec::new());
        let mut record = Record::new(writer.schema()).unwrap();
        record.put("my_field1", 123);
        let mut encoded = to_avro_datum(&schema, record).unwrap();
        // add 1 magic byte + 4 id bytes
        let mut with_header: Vec<u8> = vec![0x00, 0x00, 0x00, 0x00, 0x00];
        with_header.append(&mut encoded);
        //let res = parse_avro(&with_header[..], &schema).unwrap();
        // [0, 0, 1, 134, 197, 246, 1]
        // [0, 1, 134, 197] -> 0x01, 0x86, 0xC5 -> 100037
        // [0, 0, 0, 0, 246, 1]
        //assert_eq!(res, r#"{"my_field1":123}"#)
    }

    #[test]
    fn u8_array_to_i32() {
        let raw: Vec<u8> = vec![0x00, 0x00, 0x01, 0x86, 0xc5, 0x00, 0x00, 0x00];
        let id = get_schema_id(&raw).unwrap();
        assert_eq!(id, 100037)
    }
}