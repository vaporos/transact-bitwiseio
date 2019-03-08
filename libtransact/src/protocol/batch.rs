use hex;
use protobuf::Message;
use std;
use std::error::Error as StdError;

use crate::protos;
use crate::protos::{FromNative, FromProto, IntoNative, IntoProto, ProtoConversionError};
use crate::signing;

use super::transaction::Transaction;

#[derive(Clone)]
pub struct BatchHeader {
    signer_public_key: Vec<u8>,
    transaction_ids: Vec<Vec<u8>>,
}

impl BatchHeader {
    pub fn signer_public_key(&self) -> &[u8] {
        &self.signer_public_key
    }

    pub fn transaction_ids(&self) -> &[Vec<u8>] {
        &self.transaction_ids
    }
}

impl FromProto<protos::batch::BatchHeader> for BatchHeader {
    fn from_proto(header: protos::batch::BatchHeader) -> Result<Self, ProtoConversionError> {
        Ok(BatchHeader {
            signer_public_key: hex::decode(header.get_signer_public_key())?,
            transaction_ids: header
                .get_transaction_ids()
                .to_vec()
                .into_iter()
                .map(|t| hex::decode(t).map_err(ProtoConversionError::from))
                .collect::<Result<_, _>>()?,
        })
    }
}

impl FromNative<BatchHeader> for protos::batch::BatchHeader {
    fn from_native(header: BatchHeader) -> Result<Self, ProtoConversionError> {
        let mut proto_header = protos::batch::BatchHeader::new();
        proto_header.set_signer_public_key(hex::encode(header.signer_public_key));
        proto_header.set_transaction_ids(
            header
                .transaction_ids
                .iter()
                .map(hex::encode)
                .collect::<protobuf::RepeatedField<String>>(),
        );
        Ok(proto_header)
    }
}

impl IntoProto<protos::batch::BatchHeader> for BatchHeader {}
impl IntoNative<BatchHeader> for protos::batch::BatchHeader {}

pub struct Batch {
    header: Vec<u8>,
    header_signature: String,
    transactions: Vec<Transaction>,
    trace: bool,
}

impl Batch {
    pub fn header(&self) -> &[u8] {
        &self.header
    }

    pub fn header_signature(&self) -> &str {
        &self.header_signature
    }

    pub fn transactions(&self) -> &[Transaction] {
        &self.transactions
    }

    pub fn trace(&self) -> bool {
        self.trace
    }
}

pub struct BatchPair {
    batch: Batch,
    header: BatchHeader,
}

impl BatchPair {
    pub fn batch(&self) -> &Batch {
        &self.batch
    }

    pub fn header(&self) -> &BatchHeader {
        &self.header
    }

    pub fn take(self) -> (Batch, BatchHeader) {
        (self.batch, self.header)
    }
}

impl From<protos::batch::Batch> for Batch {
    fn from(batch: protos::batch::Batch) -> Self {
        Batch {
            header: batch.get_header().to_vec(),
            header_signature: batch.get_header_signature().to_string(),
            transactions: batch
                .get_transactions()
                .to_vec()
                .into_iter()
                .map(Transaction::from)
                .collect(),
            trace: batch.get_trace(),
        }
    }
}

#[derive(Debug)]
pub enum BatchBuildError {
    MissingField(String),
    SerializationError(String),
    SigningError(String),
}

impl StdError for BatchBuildError {
    fn description(&self) -> &str {
        match *self {
            BatchBuildError::MissingField(ref msg) => msg,
            BatchBuildError::SerializationError(ref msg) => msg,
            BatchBuildError::SigningError(ref msg) => msg,
        }
    }

    fn cause(&self) -> Option<&StdError> {
        match *self {
            BatchBuildError::MissingField(_) => None,
            BatchBuildError::SerializationError(_) => None,
            BatchBuildError::SigningError(_) => None,
        }
    }
}

impl std::fmt::Display for BatchBuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match *self {
            BatchBuildError::MissingField(ref s) => write!(f, "MissingField: {}", s),
            BatchBuildError::SerializationError(ref s) => write!(f, "SerializationError: {}", s),
            BatchBuildError::SigningError(ref s) => write!(f, "SigningError: {}", s),
        }
    }
}

#[derive(Default, Clone)]
pub struct BatchBuilder {
    transactions: Option<Vec<Transaction>>,
    trace: Option<bool>,
}

impl BatchBuilder {
    pub fn new() -> Self {
        BatchBuilder::default()
    }

    pub fn with_transactions(mut self, transactions: Vec<Transaction>) -> BatchBuilder {
        self.transactions = Some(transactions);
        self
    }

    pub fn with_trace(mut self, trace: bool) -> BatchBuilder {
        self.trace = Some(trace);
        self
    }

    pub fn build_pair(self, signer: &signing::Signer) -> Result<BatchPair, BatchBuildError> {
        let transactions = self.transactions.ok_or_else(|| {
            BatchBuildError::MissingField("'transactions' field is required".to_string())
        })?;
        let trace = self.trace.unwrap_or(false);
        let transaction_ids = transactions
            .iter()
            .flat_map(|t| {
                vec![hex::decode(t.header_signature())
                    .map_err(|e| BatchBuildError::SerializationError(format!("{}", e)))]
            })
            .collect::<Result<_, _>>()?;

        let signer_public_key = signer.public_key().to_vec();

        let header = BatchHeader {
            signer_public_key,
            transaction_ids,
        };

        let header_proto: protos::batch::BatchHeader = header
            .clone()
            .into_proto()
            .map_err(|e| BatchBuildError::SerializationError(format!("{}", e)))?;
        let header_bytes = header_proto
            .write_to_bytes()
            .map_err(|e| BatchBuildError::SerializationError(format!("{}", e)))?;

        let header_signature = hex::encode(
            signer
                .sign(&header_bytes)
                .map_err(|e| BatchBuildError::SigningError(format!("{}", e)))?,
        );

        let batch = Batch {
            header: header_bytes,
            header_signature,
            transactions,
            trace,
        };

        Ok(BatchPair { batch, header })
    }

    pub fn build(self, signer: &signing::Signer) -> Result<Batch, BatchBuildError> {
        Ok(self.build_pair(signer)?.batch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signing::hash::HashSigner;
    use crate::signing::Signer;
    use protobuf::Message;
    use sawtooth_sdk;

    static KEY1: &str = "111111111111111111111111111111111111111111111111111111111111111111";
    static KEY2: &str = "222222222222222222222222222222222222222222222222222222222222222222";
    static KEY3: &str = "333333333333333333333333333333333333333333333333333333333333333333";
    static BYTES1: [u8; 4] = [0x01, 0x02, 0x03, 0x04];
    static BYTES2: [u8; 4] = [0x05, 0x06, 0x07, 0x08];
    static BYTES3: [u8; 4] = [0x09, 0x0a, 0x0b, 0x0c];
    static BYTES4: [u8; 4] = [0x0d, 0x0e, 0x0f, 0x10];
    static BYTES5: [u8; 4] = [0x11, 0x12, 0x13, 0x14];
    static SIGNATURE1: &str =
        "sig1sig1sig1sig1sig1sig1sig1sig1sig1sig1sig1sig1sig1sig1sig1sig1sig1sig1";
    static SIGNATURE2: &str =
        "sig2sig2sig2sig2sig2sig2sig2sig2sig2sig2sig2sig2sig2sig2sig2sig2sig2sig2";
    static SIGNATURE3: &str =
        "sig3sig3sig3sig3sig3sig3sig3sig3sig3sig3sig3sig3sig3sig3sig3sig3sig3sig3";

    fn check_builder_batch(signer: &Signer, pair: &BatchPair) {
        assert_eq!(
            vec![
                SIGNATURE2.as_bytes().to_vec(),
                SIGNATURE3.as_bytes().to_vec()
            ],
            pair.header().transaction_ids()
        );
        assert_eq!(signer.public_key(), pair.header().signer_public_key());
        assert_eq!(
            vec![
                Transaction::new(
                    BYTES2.to_vec(),
                    hex::encode(SIGNATURE2.to_string()),
                    BYTES3.to_vec()
                ),
                Transaction::new(
                    BYTES4.to_vec(),
                    hex::encode(SIGNATURE3.to_string()),
                    BYTES5.to_vec()
                ),
            ],
            pair.batch().transactions()
        );
        assert_eq!(true, pair.batch().trace());
    }

    #[test]
    fn batch_builder_chain() {
        let signer = HashSigner::new();

        let pair = BatchBuilder::new()
            .with_transactions(vec![
                Transaction::new(
                    BYTES2.to_vec(),
                    hex::encode(SIGNATURE2.to_string()),
                    BYTES3.to_vec(),
                ),
                Transaction::new(
                    BYTES4.to_vec(),
                    hex::encode(SIGNATURE3.to_string()),
                    BYTES5.to_vec(),
                ),
            ])
            .with_trace(true)
            .build_pair(&signer)
            .unwrap();

        check_builder_batch(&signer, &pair);
    }

    #[test]
    fn batch_builder_separate() {
        let signer = HashSigner::new();

        let mut builder = BatchBuilder::new();
        builder = builder.with_transactions(vec![
            Transaction::new(
                BYTES2.to_vec(),
                hex::encode(SIGNATURE2.to_string()),
                BYTES3.to_vec(),
            ),
            Transaction::new(
                BYTES4.to_vec(),
                hex::encode(SIGNATURE3.to_string()),
                BYTES5.to_vec(),
            ),
        ]);
        builder = builder.with_trace(true);
        let pair = builder.build_pair(&signer).unwrap();

        check_builder_batch(&signer, &pair);
    }

    #[test]
    fn batch_header_fields() {
        let header = BatchHeader {
            signer_public_key: hex::decode(KEY1).unwrap(),
            transaction_ids: vec![hex::decode(KEY2).unwrap(), hex::decode(KEY3).unwrap()],
        };

        assert_eq!(KEY1, hex::encode(header.signer_public_key()));
        assert_eq!(
            vec![hex::decode(KEY2).unwrap(), hex::decode(KEY3).unwrap(),],
            header.transaction_ids()
        );
    }

    #[test]
    fn batch_header_sawtooth10_compatibility() {
        // Create protobuf bytes using the Sawtooth SDK
        let mut proto = sawtooth_sdk::messages::batch::BatchHeader::new();
        proto.set_signer_public_key(KEY1.to_string());
        proto.set_transaction_ids(protobuf::RepeatedField::from_vec(vec![
            KEY2.to_string(),
            KEY3.to_string(),
        ]));
        let header_bytes = proto.write_to_bytes().unwrap();

        // Deserialize the header bytes into our protobuf
        let header_proto: protos::batch::BatchHeader =
            protobuf::parse_from_bytes(&header_bytes).unwrap();

        // Convert to a BatchHeader
        let header: BatchHeader = header_proto.into_native().unwrap();

        assert_eq!(KEY1, hex::encode(header.signer_public_key()));
        assert_eq!(
            vec![hex::decode(KEY2).unwrap(), hex::decode(KEY3).unwrap(),],
            header.transaction_ids(),
        );
    }

    #[test]
    fn batch_fields() {
        let batch = Batch {
            header: BYTES1.to_vec(),
            header_signature: SIGNATURE1.to_string(),
            transactions: vec![
                Transaction::new(BYTES2.to_vec(), SIGNATURE2.to_string(), BYTES3.to_vec()),
                Transaction::new(BYTES4.to_vec(), SIGNATURE3.to_string(), BYTES5.to_vec()),
            ],
            trace: true,
        };

        assert_eq!(BYTES1.to_vec(), batch.header());
        assert_eq!(SIGNATURE1, batch.header_signature());
        assert_eq!(
            vec![
                Transaction::new(BYTES2.to_vec(), SIGNATURE2.to_string(), BYTES3.to_vec()),
                Transaction::new(BYTES4.to_vec(), SIGNATURE3.to_string(), BYTES5.to_vec()),
            ],
            batch.transactions()
        );
        assert_eq!(true, batch.trace());
    }

    #[test]
    fn batch_sawtooth10_compatibility() {}
}

#[cfg(all(feature = "nightly", test))]
mod benchmarks {
    extern crate test;
    use super::*;
    use crate::signing::hash::HashSigner;
    use test::Bencher;

    static KEY1: &str = "111111111111111111111111111111111111111111111111111111111111111111";
    static KEY2: &str = "222222222222222222222222222222222222222222222222222222222222222222";
    static KEY3: &str = "333333333333333333333333333333333333333333333333333333333333333333";
    static BYTES1: [u8; 4] = [0x01, 0x02, 0x03, 0x04];
    static BYTES2: [u8; 4] = [0x05, 0x06, 0x07, 0x08];
    static BYTES3: [u8; 4] = [0x09, 0x0a, 0x0b, 0x0c];
    static BYTES4: [u8; 4] = [0x0d, 0x0e, 0x0f, 0x10];
    static BYTES5: [u8; 4] = [0x11, 0x12, 0x13, 0x14];
    static SIGNATURE1: &str =
        "sig1sig1sig1sig1sig1sig1sig1sig1sig1sig1sig1sig1sig1sig1sig1sig1sig1sig1";
    static SIGNATURE2: &str =
        "sig2sig2sig2sig2sig2sig2sig2sig2sig2sig2sig2sig2sig2sig2sig2sig2sig2sig2";
    static SIGNATURE3: &str =
        "sig3sig3sig3sig3sig3sig3sig3sig3sig3sig3sig3sig3sig3sig3sig3sig3sig3sig3";

    #[bench]
    fn bench_batch_creation(b: &mut Bencher) {
        b.iter(|| Batch {
            header: BYTES1.to_vec(),
            header_signature: SIGNATURE1.to_string(),
            transactions: vec![
                Transaction::new(BYTES2.to_vec(), SIGNATURE2.to_string(), BYTES3.to_vec()),
                Transaction::new(BYTES4.to_vec(), SIGNATURE3.to_string(), BYTES5.to_vec()),
            ],
            trace: true,
        });
    }

    #[bench]
    fn bench_batch_builder(b: &mut Bencher) {
        let signer = HashSigner::new();
        let batch = BatchBuilder::new()
            .with_transactions(vec![
                Transaction::new(
                    BYTES2.to_vec(),
                    hex::encode(SIGNATURE2.to_string()),
                    BYTES3.to_vec(),
                ),
                Transaction::new(
                    BYTES4.to_vec(),
                    hex::encode(SIGNATURE3.to_string()),
                    BYTES5.to_vec(),
                ),
            ])
            .with_trace(true);
        b.iter(|| batch.clone().build_pair(&signer));
    }

    #[bench]
    fn bench_batch_header_into_native(b: &mut Bencher) {
        let mut proto_header = protos::batch::BatchHeader::new();
        proto_header.set_signer_public_key(KEY1.to_string());
        proto_header.set_transaction_ids(protobuf::RepeatedField::from_vec(vec![
            KEY2.to_string(),
            KEY3.to_string(),
        ]));
        b.iter(|| proto_header.clone().into_native());
    }

    #[bench]
    fn bench_batch_header_into_proto(b: &mut Bencher) {
        let native_header = BatchHeader {
            signer_public_key: hex::decode(KEY1).unwrap(),
            transaction_ids: vec![hex::decode(KEY2).unwrap(), hex::decode(KEY3).unwrap()],
        };
        b.iter(|| native_header.clone().into_proto());
    }
}
