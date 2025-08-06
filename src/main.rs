use ahash::{HashMap, HashMapExt, HashSet, HashSetExt};
use anyhow::{Result, anyhow};
use clap::Parser;
use csv;
use rust_decimal::Decimal;
use serde::Deserialize;
use std::fs::File;

#[derive(Parser)]
struct Opts {
    filename: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum TransactionType {
    Deposit,
    Withdrawal,
    Dispute,
    Resolve,
    Chargeback,
}

#[derive(Debug, Deserialize)]
struct Transaction {
    #[serde(rename = "type")]
    kind: TransactionType,
    #[serde(rename = "client")]
    client_id: u16,
    #[serde(rename = "tx")]
    id: u32,
    amount: Option<Decimal>,
}

#[derive(Debug, Default)]
struct Client {
    available_funds: Decimal,
    held_funds: Decimal,
    total_funds: Decimal,
    locked: bool,
}

#[derive(Debug)]
struct TransactionRecord {
    client_id: u16,
    amount: Decimal,
}

fn main() -> Result<()> {
    let opts = Opts::parse();
    let file = File::open(&opts.filename)?;

    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(file);
    let records = reader
        .deserialize::<Transaction>()
        .map(|r| r.map_err(Into::into));

    let clients = process_transactions(records)?;

    //Output client data
    println!("client,available,held,total,locked");
    for (client_id, client) in clients {
        println!(
            "{},{:.4},{:.4},{:.4},{}",
            client_id, client.available_funds, client.held_funds, client.total_funds, client.locked
        );
    }

    Ok(())
}

fn process_transactions<T>(records: T) -> Result<HashMap<u16, Client>>
where
    T: IntoIterator<Item = Result<Transaction>>,
{
    let mut clients: HashMap<u16, Client> = HashMap::new();
    let mut transaction_records: HashMap<u32, TransactionRecord> = HashMap::new();
    let mut disputed_transaction: HashSet<u32> = HashSet::new();

    // Sanitize inputs

    for r in records {
        let record = r?;
        let client = clients.entry(record.client_id).or_default();

        // Ignore all transactions from locked client
        if client.locked {
            continue;
        }
        // Convert all if conditions above to improve
        // readability
        match record.kind {
            TransactionType::Deposit => {
                if let Some(a) = record.amount {
                    client.available_funds += a;
                    client.total_funds += a;
                    transaction_records.insert(
                        record.id,
                        TransactionRecord {
                            client_id: record.client_id,
                            amount: a,
                        },
                    );
                } // Record some error
            }
            TransactionType::Withdrawal => {
                if let Some(a) = record.amount {
                    // Sufficient funds available
                    if client.available_funds < a {
                        continue;
                    }
                    client.available_funds -= a;
                    client.total_funds -= a;
                } // record some error
            }
            TransactionType::Dispute => {
                // Make sure if there is no double disputes open
                if disputed_transaction.contains(&record.id) {
                    continue;
                }

                if let Some(tr) = transaction_records.get(&record.id) {
                    // Check for malicious client
                    if tr.client_id == record.client_id {
                        // Make sure client has enough funds
                        if client.available_funds < tr.amount {
                            continue;
                        }
                        // Update the funds
                        client.available_funds -= tr.amount;
                        client.held_funds += tr.amount;

                        // Record the transaction id under dispute
                        disputed_transaction.insert(record.id);
                    }
                }
            }
            TransactionType::Resolve => {
                // Ignore if transaction not disputed
                if !disputed_transaction.contains(&record.id) {
                    continue;
                }

                if let Some(tr) = transaction_records.get(&record.id) {
                    if tr.client_id != record.client_id {
                        // Malicious actor
                        continue;
                    }
                    // Update the funds
                    client.available_funds += tr.amount;
                    client.held_funds -= tr.amount;

                    // Remove the disputed transaction
                    disputed_transaction.remove(&record.id);
                }
            }
            TransactionType::Chargeback => {
                // Ignore if transaction not disputed
                if !disputed_transaction.contains(&record.id) {
                    continue;
                }

                if let Some(tr) = transaction_records.get(&record.id) {
                    // Update the funds
                    client.held_funds -= tr.amount;
                    client.total_funds -= tr.amount;
                    // Lock the client
                    client.locked = true;

                    // Remove the disputed transaction
                    disputed_transaction.remove(&record.id);
                }
            }
        }
    }
    Ok(clients)
}
