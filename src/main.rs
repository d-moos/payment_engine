use crate::balance::Amount;
use crate::client::{Client, ClientId, TransactionId};
use crate::payment_engine::{PaymentEngine, Transaction, TransactionType};
use csv::Trim::All;
use csv::{ReaderBuilder, WriterBuilder};
use log::warn;
use serde::{Deserialize, Serialize};
use std::env::args;
use std::fs::File;
use std::io;
use std::io::BufReader;

mod balance;
mod client;
mod payment_engine;

const SCALE: f64 = 10000f64;

#[derive(Debug, Deserialize)]
struct CsvTransactionItem {
    r#type: String,
    client: ClientId,
    tx: TransactionId,
    amount: Option<f64>,
}

impl Into<Transaction> for CsvTransactionItem {
    fn into(self) -> Transaction {
        let transaction_type = match self.r#type.as_str() {
            "deposit" => {
                TransactionType::Deposit((self.amount.unwrap() * SCALE).round() as Amount)
            }
            "withdrawal" => {
                TransactionType::Withdrawal((self.amount.unwrap() * SCALE).round() as Amount)
            }
            "dispute" => TransactionType::Dispute,
            "resolve" => TransactionType::Resolve,
            "chargeback" => TransactionType::Chargeback,
            _ => panic!("invalid transaction type found"),
        };

        Transaction::new(self.tx, self.client, transaction_type)
    }
}

#[derive(Debug, Serialize)]
struct CsvClientItem {
    client: ClientId,
    available: f64,
    held: f64,
    total: f64,
    locked: bool,
}

impl From<Client> for CsvClientItem {
    fn from(value: Client) -> Self {
        let available = value.balance().available() as f64;
        let frozen = value.balance().frozen() as f64;

        // this should be safe as the engine makes sure that total is always in range of a u64.
        let total = available + frozen;
        Self {
            client: value.id(),
            available: available / SCALE,
            held: frozen / SCALE,
            total: total / SCALE,
            locked: value.is_locked(),
        }
    }
}

fn main() {
    env_logger::init();

    let input_file = args()
        .nth(1)
        .expect("input file missing! call: cargo run -- [FILE].csv");
    let file = File::open(input_file).expect("could not open given input file");
    let buffered_reader = BufReader::new(file);
    let mut csv_reader = ReaderBuilder::new().trim(All).from_reader(buffered_reader);

    let mut engine = PaymentEngine::default();

    for deserialized_item in csv_reader.deserialize::<CsvTransactionItem>() {
        if let Ok(item) = deserialized_item {
            if let Err(e) = engine.execute(item.into()) {
                warn!("transaction failed to execute: {:?}", e);
            }
        } else {
            warn!("failed parsing csv line");
        }
    }

    let mut writer = WriterBuilder::new().from_writer(io::stdout());
    for x in engine.into_clients() {
        let item: CsvClientItem = x.into();
        writer.serialize(item).unwrap();
    }
}
