use ahash::{HashMap, HashMapExt, HashSet, HashSetExt};
use anyhow::{Result, anyhow};
use clap::Parser;
use csv::{ReaderBuilder, Trim};
use rust_decimal::Decimal;
use serde::Deserialize;
use std::fs::File;

#[derive(Parser)]
struct Opts {
    filename: String,
}

#[derive(Debug, Deserialize, PartialEq)]
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
    transaction_type: TransactionType,
}

fn main() -> Result<()> {
    let opts = Opts::parse();
    let file = File::open(&opts.filename)?;

    let mut reader = ReaderBuilder::new()
        .flexible(true)
        .trim(Trim::All)
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

    for record in records {
        let current_transaction = match record {
            Ok(r) => r,
            Err(e) => {
                //log error
                continue;
            }
        };
        let client = clients.entry(current_transaction.client_id).or_default();

        // Ignore all transactions from locked client
        if client.locked {
            continue;
        }
        // Convert all if conditions above to improve
        // readability
        match current_transaction.kind {
            TransactionType::Deposit => {
                if transaction_records.contains_key(&current_transaction.id) {
                    // This transaction ID has been used before
                    // There is some error
                    continue;
                }

                let amount = if let Some(a) = current_transaction.amount {
                    a
                } else {
                    continue;
                };

                client.available_funds += amount;
                client.total_funds += amount;
                transaction_records.insert(
                    current_transaction.id,
                    TransactionRecord {
                        client_id: current_transaction.client_id,
                        amount,
                        transaction_type: current_transaction.kind,
                    },
                );
            }
            TransactionType::Withdrawal => {
                if transaction_records.contains_key(&current_transaction.id) {
                    // This transaction ID has been used before
                    // There is some error
                    continue;
                }

                let amount = if let Some(a) = current_transaction.amount {
                    a
                } else {
                    continue;
                };
                // Sufficient funds available
                if client.available_funds < amount {
                    continue;
                }
                client.available_funds -= amount;
                client.total_funds -= amount;

                transaction_records.insert(
                    current_transaction.id,
                    TransactionRecord {
                        client_id: current_transaction.client_id,
                        amount,
                        transaction_type: current_transaction.kind,
                    },
                );
                // record some error
            }
            TransactionType::Dispute => {
                // Make sure if there is no double disputes open
                if disputed_transaction.contains(&current_transaction.id) {
                    continue;
                }

                // Check if transaction to be disputed exists
                let transaction_record =
                    if let Some(tr) = transaction_records.get(&current_transaction.id) {
                        tr
                    } else {
                        continue;
                    };

                // Check for malicious client
                if transaction_record.client_id != current_transaction.client_id
                    || transaction_record.transaction_type != TransactionType::Deposit
                {
                    continue;
                }

                // Make sure client has enough funds
                if client.available_funds < transaction_record.amount {
                    continue;
                }

                // Update the funds
                client.available_funds -= transaction_record.amount;
                client.held_funds += transaction_record.amount;

                // Record the transaction id under dispute
                disputed_transaction.insert(current_transaction.id);
            }
            TransactionType::Resolve => {
                // Ignore if transaction not disputed
                if !disputed_transaction.contains(&current_transaction.id) {
                    continue;
                }

                let transaction_record =
                    if let Some(tr) = transaction_records.get(&current_transaction.id) {
                        tr
                    } else {
                        continue;
                    };

                if transaction_record.client_id != current_transaction.client_id {
                    // Malicious actor
                    continue;
                }
                // Update the funds
                client.available_funds += transaction_record.amount;
                client.held_funds -= transaction_record.amount;

                // Remove the disputed transaction
                disputed_transaction.remove(&current_transaction.id);
            }
            TransactionType::Chargeback => {
                // Ignore if transaction not disputed
                if !disputed_transaction.contains(&current_transaction.id) {
                    continue;
                }

                let transaction_record =
                    if let Some(tr) = transaction_records.get(&current_transaction.id) {
                        tr
                    } else {
                        continue;
                    };

                // Update the funds
                client.held_funds -= transaction_record.amount;
                client.total_funds -= transaction_record.amount;
                // Lock the client
                client.locked = true;

                // Remove the disputed transaction
                disputed_transaction.remove(&current_transaction.id);
            }
        }
    }
    Ok(clients)
}

#[cfg(test)]
mod tests {
    use super::*;

    use rust_decimal::dec;

    #[test]
    fn test_deposit_funds_multiple_clients() {
        let records = vec![
            Ok(Transaction {
                kind: TransactionType::Deposit,
                client_id: 1,
                id: 1,
                amount: Some(dec!(1.234)),
            }),
            Ok(Transaction {
                kind: TransactionType::Deposit,
                client_id: 1,
                id: 3,
                amount: Some(dec!(12.34)),
            }),
            Ok(Transaction {
                kind: TransactionType::Deposit,
                client_id: 2,
                id: 2,
                amount: Some(dec!(0.1234)),
            }),
            Ok(Transaction {
                kind: TransactionType::Deposit,
                client_id: 2,
                id: 4,
                amount: Some(dec!(12.34)),
            }),
            Ok(Transaction {
                kind: TransactionType::Deposit,
                client_id: 1,
                id: 5,
                amount: Some(dec!(0.1234)),
            }),
        ];

        let clients = process_transactions(records).unwrap();
        let client_1 = clients.get(&1).unwrap();

        assert_eq!(client_1.available_funds, dec!(13.6974));
        assert_eq!(client_1.total_funds, dec!(13.6974));
        assert_eq!(client_1.held_funds, dec!(0));
        assert!(!client_1.locked);

        let client_2 = clients.get(&2).unwrap();

        assert_eq!(client_2.available_funds, dec!(12.4634));
        assert_eq!(client_2.total_funds, dec!(12.4634));
        assert_eq!(client_2.held_funds, dec!(0));
        assert!(!client_2.locked);
    }

    #[test]
    fn test_withdraw_funds_multiple_clients() {
        let records = vec![
            Ok(Transaction {
                kind: TransactionType::Deposit,
                client_id: 1,
                id: 1,
                amount: Some(dec!(123.4)),
            }),
            Ok(Transaction {
                kind: TransactionType::Deposit,
                client_id: 2,
                id: 2,
                amount: Some(dec!(12.56)),
            }),
            Ok(Transaction {
                kind: TransactionType::Withdrawal,
                client_id: 2,
                id: 3,
                amount: Some(dec!(0.1234)),
            }),
            Ok(Transaction {
                kind: TransactionType::Withdrawal,
                client_id: 2,
                id: 4,
                amount: Some(dec!(12.34)),
            }),
            Ok(Transaction {
                kind: TransactionType::Withdrawal,
                client_id: 1,
                id: 5,
                amount: Some(dec!(1.234)),
            }),
        ];

        let clients = process_transactions(records).unwrap();
        let client_1 = clients.get(&1).unwrap();

        assert_eq!(client_1.available_funds, dec!(122.166));
        assert_eq!(client_1.total_funds, dec!(122.166));
        assert_eq!(client_1.held_funds, dec!(0));
        assert!(!client_1.locked);

        let client_2 = clients.get(&2).unwrap();

        assert_eq!(client_2.available_funds, dec!(0.0966));
        assert_eq!(client_2.total_funds, dec!(0.0966));
        assert_eq!(client_2.held_funds, dec!(0));
        assert!(!client_2.locked);
    }

    #[test]
    fn test_withdraw_from_insufficient_balance() {
        let records = vec![
            Ok(Transaction {
                kind: TransactionType::Deposit,
                client_id: 1,
                id: 1,
                amount: Some(dec!(12.34)),
            }),
            Ok(Transaction {
                kind: TransactionType::Withdrawal,
                client_id: 1,
                id: 2,
                amount: Some(dec!(1.256)),
            }),
            Ok(Transaction {
                kind: TransactionType::Withdrawal,
                client_id: 1,
                id: 5,
                amount: Some(dec!(123.4)),
            }),
        ];

        let clients = process_transactions(records).unwrap();
        let client_1 = clients.get(&1).unwrap();

        assert_eq!(client_1.available_funds, dec!(11.084));
        assert_eq!(client_1.total_funds, dec!(11.084));
        assert_eq!(client_1.held_funds, dec!(0));
        assert!(!client_1.locked);
    }

    #[test]
    fn test_transaction_id_repeated_for_withdraw() {
        let records = vec![
            Ok(Transaction {
                kind: TransactionType::Deposit,
                client_id: 1,
                id: 1,
                amount: Some(dec!(12.34)),
            }),
            Ok(Transaction {
                kind: TransactionType::Withdrawal,
                client_id: 1,
                id: 2,
                amount: Some(dec!(1.256)),
            }),
            Ok(Transaction {
                kind: TransactionType::Withdrawal,
                client_id: 1,
                id: 2,
                amount: Some(dec!(0.1234)),
            }),
        ];

        let clients = process_transactions(records).unwrap();
        let client_1 = clients.get(&1).unwrap();

        assert_eq!(client_1.available_funds, dec!(11.084));
        assert_eq!(client_1.total_funds, dec!(11.084));
        assert_eq!(client_1.held_funds, dec!(0));
        assert!(!client_1.locked);
    }

    #[test]
    fn test_transaction_id_repeated_for_deposit() {
        let records = vec![
            Ok(Transaction {
                kind: TransactionType::Deposit,
                client_id: 1,
                id: 1,
                amount: Some(dec!(12.34)),
            }),
            Ok(Transaction {
                kind: TransactionType::Deposit,
                client_id: 1,
                id: 1,
                amount: Some(dec!(1.256)),
            }),
        ];

        let clients = process_transactions(records).unwrap();
        let client_1 = clients.get(&1).unwrap();

        assert_eq!(client_1.available_funds, dec!(12.34));
        assert_eq!(client_1.total_funds, dec!(12.34));
        assert_eq!(client_1.held_funds, dec!(0));
        assert!(!client_1.locked);
    }

    #[test]
    fn test_open_dispute_for_transaction() {
        let records = vec![
            Ok(Transaction {
                kind: TransactionType::Deposit,
                client_id: 1,
                id: 1,
                amount: Some(dec!(12.34)),
            }),
            Ok(Transaction {
                kind: TransactionType::Deposit,
                client_id: 1,
                id: 2,
                amount: Some(dec!(1.256)),
            }),
            Ok(Transaction {
                kind: TransactionType::Dispute,
                client_id: 1,
                id: 1,
                amount: None,
            }),
        ];

        let clients = process_transactions(records).unwrap();
        let client_1 = clients.get(&1).unwrap();

        assert_eq!(client_1.available_funds, dec!(1.256));
        assert_eq!(client_1.total_funds, dec!(13.596));
        assert_eq!(client_1.held_funds, dec!(12.34));
        assert!(!client_1.locked);
    }

    #[test]
    fn test_open_dispute_with_insufficient_funds() {
        let records = vec![
            Ok(Transaction {
                kind: TransactionType::Deposit,
                client_id: 1,
                id: 1,
                amount: Some(dec!(12.34)),
            }),
            Ok(Transaction {
                kind: TransactionType::Withdrawal,
                client_id: 1,
                id: 2,
                amount: Some(dec!(1.234)),
            }),
            Ok(Transaction {
                kind: TransactionType::Dispute,
                client_id: 1,
                id: 1,
                amount: None,
            }),
        ];

        let clients = process_transactions(records).unwrap();
        let client_1 = clients.get(&1).unwrap();

        assert_eq!(client_1.available_funds, dec!(11.106));
        assert_eq!(client_1.total_funds, dec!(11.106));
        assert_eq!(client_1.held_funds, dec!(0));
        assert!(!client_1.locked);
    }

    #[test]
    fn test_resolve_opened_dispute() {
        let records = vec![
            Ok(Transaction {
                kind: TransactionType::Deposit,
                client_id: 1,
                id: 1,
                amount: Some(dec!(12.34)),
            }),
            Ok(Transaction {
                kind: TransactionType::Dispute,
                client_id: 1,
                id: 1,
                amount: None,
            }),
            Ok(Transaction {
                kind: TransactionType::Resolve,
                client_id: 1,
                id: 1,
                amount: None,
            }),
        ];

        let clients = process_transactions(records).unwrap();
        let client_1 = clients.get(&1).unwrap();

        assert_eq!(client_1.available_funds, dec!(12.34));
        assert_eq!(client_1.total_funds, dec!(12.34));
        assert_eq!(client_1.held_funds, dec!(0));
        assert!(!client_1.locked);
    }

    #[test]
    fn test_chargeback_opened_dispute() {
        let records = vec![
            Ok(Transaction {
                kind: TransactionType::Deposit,
                client_id: 1,
                id: 1,
                amount: Some(dec!(12.34)),
            }),
            Ok(Transaction {
                kind: TransactionType::Deposit,
                client_id: 1,
                id: 3,
                amount: Some(dec!(0.1234)),
            }),
            Ok(Transaction {
                kind: TransactionType::Deposit,
                client_id: 1,
                id: 2,
                amount: Some(dec!(1.234)),
            }),
            Ok(Transaction {
                kind: TransactionType::Dispute,
                client_id: 1,
                id: 1,
                amount: None,
            }),
            Ok(Transaction {
                kind: TransactionType::Dispute,
                client_id: 1,
                id: 2,
                amount: None,
            }),
            Ok(Transaction {
                kind: TransactionType::Chargeback,
                client_id: 1,
                id: 1,
                amount: None,
            }),
        ];

        let clients = process_transactions(records).unwrap();
        let client_1 = clients.get(&1).unwrap();

        assert_eq!(client_1.available_funds, dec!(0.1234));
        assert_eq!(client_1.total_funds, dec!(1.3574));
        assert_eq!(client_1.held_funds, dec!(1.234));
        assert!(client_1.locked);
    }

    #[test]
    fn test_transactions_after_account_locked() {
        let records = vec![
            Ok(Transaction {
                kind: TransactionType::Deposit,
                client_id: 1,
                id: 1,
                amount: Some(dec!(12.34)),
            }),
            Ok(Transaction {
                kind: TransactionType::Deposit,
                client_id: 1,
                id: 2,
                amount: Some(dec!(1.234)),
            }),
            Ok(Transaction {
                kind: TransactionType::Dispute,
                client_id: 1,
                id: 2,
                amount: None,
            }),
            Ok(Transaction {
                kind: TransactionType::Chargeback,
                client_id: 1,
                id: 2,
                amount: None,
            }),
            Ok(Transaction {
                kind: TransactionType::Deposit,
                client_id: 1,
                id: 4,
                amount: Some(dec!(65.78)),
            }),
            Ok(Transaction {
                kind: TransactionType::Withdrawal,
                client_id: 1,
                id: 3,
                amount: Some(dec!(6.578)),
            }),
            Ok(Transaction {
                kind: TransactionType::Dispute,
                client_id: 1,
                id: 3,
                amount: None,
            }),
        ];

        let clients = process_transactions(records).unwrap();
        let client_1 = clients.get(&1).unwrap();

        assert_eq!(client_1.available_funds, dec!(12.34));
        assert_eq!(client_1.total_funds, dec!(12.34));
        assert_eq!(client_1.held_funds, dec!(0));
        assert!(client_1.locked);
    }

    #[test]
    fn test_ignore_chargeback_if_not_disputed() {
        let records = vec![
            Ok(Transaction {
                kind: TransactionType::Deposit,
                client_id: 1,
                id: 1,
                amount: Some(dec!(12.34)),
            }),
            Ok(Transaction {
                kind: TransactionType::Deposit,
                client_id: 1,
                id: 2,
                amount: Some(dec!(1.234)),
            }),
            Ok(Transaction {
                kind: TransactionType::Chargeback,
                client_id: 1,
                id: 2,
                amount: None,
            }),
        ];

        let clients = process_transactions(records).unwrap();
        let client_1 = clients.get(&1).unwrap();

        assert_eq!(client_1.available_funds, dec!(13.574));
        assert_eq!(client_1.total_funds, dec!(13.574));
        assert_eq!(client_1.held_funds, dec!(0));
        assert!(!client_1.locked);
    }

    #[test]
    fn test_ignore_resolve_if_not_disputed() {
        let records = vec![
            Ok(Transaction {
                kind: TransactionType::Deposit,
                client_id: 1,
                id: 1,
                amount: Some(dec!(12.34)),
            }),
            Ok(Transaction {
                kind: TransactionType::Deposit,
                client_id: 1,
                id: 2,
                amount: Some(dec!(1.234)),
            }),
            Ok(Transaction {
                kind: TransactionType::Resolve,
                client_id: 1,
                id: 2,
                amount: None,
            }),
        ];

        let clients = process_transactions(records).unwrap();
        let client_1 = clients.get(&1).unwrap();

        assert_eq!(client_1.available_funds, dec!(13.574));
        assert_eq!(client_1.total_funds, dec!(13.574));
        assert_eq!(client_1.held_funds, dec!(0));
        assert!(!client_1.locked);
    }

    #[test]
    fn test_ignore_dispute_if_already_disputed() {
        let records = vec![
            Ok(Transaction {
                kind: TransactionType::Deposit,
                client_id: 1,
                id: 1,
                amount: Some(dec!(12.34)),
            }),
            Ok(Transaction {
                kind: TransactionType::Deposit,
                client_id: 1,
                id: 2,
                amount: Some(dec!(1.234)),
            }),
            Ok(Transaction {
                kind: TransactionType::Dispute,
                client_id: 1,
                id: 2,
                amount: None,
            }),
            Ok(Transaction {
                kind: TransactionType::Dispute,
                client_id: 1,
                id: 2,
                amount: None,
            }),
        ];

        let clients = process_transactions(records).unwrap();
        let client_1 = clients.get(&1).unwrap();

        assert_eq!(client_1.available_funds, dec!(12.34));
        assert_eq!(client_1.total_funds, dec!(13.574));
        assert_eq!(client_1.held_funds, dec!(1.234));
        assert!(!client_1.locked);
    }

    #[test]
    fn test_ignore_dispute_if_tx_of_withdrawal() {
        let records = vec![
            Ok(Transaction {
                kind: TransactionType::Deposit,
                client_id: 1,
                id: 1,
                amount: Some(dec!(12.34)),
            }),
            Ok(Transaction {
                kind: TransactionType::Withdrawal,
                client_id: 1,
                id: 2,
                amount: Some(dec!(1.234)),
            }),
            Ok(Transaction {
                kind: TransactionType::Dispute,
                client_id: 1,
                id: 2,
                amount: None,
            }),
        ];

        let clients = process_transactions(records).unwrap();
        let client_1 = clients.get(&1).unwrap();

        assert_eq!(client_1.available_funds, dec!(11.106));
        assert_eq!(client_1.total_funds, dec!(11.106));
        assert_eq!(client_1.held_funds, dec!(0));
        assert!(!client_1.locked);
    }

    #[test]
    fn test_ignore_dispute_if_tx_and_client_dont_match() {
        let records = vec![
            Ok(Transaction {
                kind: TransactionType::Deposit,
                client_id: 1,
                id: 1,
                amount: Some(dec!(12.34)),
            }),
            Ok(Transaction {
                kind: TransactionType::Deposit,
                client_id: 2,
                id: 2,
                amount: Some(dec!(1.234)),
            }),
            Ok(Transaction {
                kind: TransactionType::Dispute,
                client_id: 1,
                id: 2,
                amount: None,
            }),
        ];

        let clients = process_transactions(records).unwrap();
        let client_1 = clients.get(&1).unwrap();

        assert_eq!(client_1.available_funds, dec!(12.34));
        assert_eq!(client_1.total_funds, dec!(12.34));
        assert_eq!(client_1.held_funds, dec!(0));
        assert!(!client_1.locked);

        let client_1 = clients.get(&2).unwrap();

        assert_eq!(client_1.available_funds, dec!(1.234));
        assert_eq!(client_1.total_funds, dec!(1.234));
        assert_eq!(client_1.held_funds, dec!(0));
        assert!(!client_1.locked);
    }

    #[test]
    fn test_ignore_resolve_if_tx_and_client_dont_match() {
        let records = vec![
            Ok(Transaction {
                kind: TransactionType::Deposit,
                client_id: 1,
                id: 1,
                amount: Some(dec!(12.34)),
            }),
            Ok(Transaction {
                kind: TransactionType::Deposit,
                client_id: 2,
                id: 2,
                amount: Some(dec!(1.234)),
            }),
            Ok(Transaction {
                kind: TransactionType::Dispute,
                client_id: 1,
                id: 1,
                amount: None,
            }),
            Ok(Transaction {
                kind: TransactionType::Resolve,
                client_id: 1,
                id: 2,
                amount: None,
            }),
        ];

        let clients = process_transactions(records).unwrap();
        let client_1 = clients.get(&1).unwrap();

        assert_eq!(client_1.available_funds, dec!(0));
        assert_eq!(client_1.total_funds, dec!(12.34));
        assert_eq!(client_1.held_funds, dec!(12.34));
        assert!(!client_1.locked);

        let client_1 = clients.get(&2).unwrap();

        assert_eq!(client_1.available_funds, dec!(1.234));
        assert_eq!(client_1.total_funds, dec!(1.234));
        assert_eq!(client_1.held_funds, dec!(0));
        assert!(!client_1.locked);
    }

    #[test]
    fn test_ignore_resolve_if_invalid_tx_id() {
        let records = vec![
            Ok(Transaction {
                kind: TransactionType::Deposit,
                client_id: 1,
                id: 1,
                amount: Some(dec!(12.34)),
            }),
            Ok(Transaction {
                kind: TransactionType::Dispute,
                client_id: 1,
                id: 1,
                amount: None,
            }),
            Ok(Transaction {
                kind: TransactionType::Resolve,
                client_id: 1,
                id: 2,
                amount: None,
            }),
        ];

        let clients = process_transactions(records).unwrap();
        let client_1 = clients.get(&1).unwrap();

        assert_eq!(client_1.available_funds, dec!(0));
        assert_eq!(client_1.total_funds, dec!(12.34));
        assert_eq!(client_1.held_funds, dec!(12.34));
        assert!(!client_1.locked);
    }

    #[test]
    fn test_ignore_dispute_if_invalid_tx_id() {
        let records = vec![
            Ok(Transaction {
                kind: TransactionType::Deposit,
                client_id: 1,
                id: 1,
                amount: Some(dec!(12.34)),
            }),
            Ok(Transaction {
                kind: TransactionType::Dispute,
                client_id: 1,
                id: 3,
                amount: None,
            }),
        ];

        let clients = process_transactions(records).unwrap();
        let client_1 = clients.get(&1).unwrap();

        assert_eq!(client_1.available_funds, dec!(12.34));
        assert_eq!(client_1.total_funds, dec!(12.34));
        assert_eq!(client_1.held_funds, dec!(0));
        assert!(!client_1.locked);
    }

    #[test]
    fn test_ignore_deposit_if_amount_is_none() {
        let records = vec![
            Ok(Transaction {
                kind: TransactionType::Deposit,
                client_id: 1,
                id: 1,
                amount: Some(dec!(12.34)),
            }),
            Ok(Transaction {
                kind: TransactionType::Deposit,
                client_id: 1,
                id: 2,
                amount: None,
            }),
        ];

        let clients = process_transactions(records).unwrap();
        let client_1 = clients.get(&1).unwrap();

        assert_eq!(client_1.available_funds, dec!(12.34));
        assert_eq!(client_1.total_funds, dec!(12.34));
        assert_eq!(client_1.held_funds, dec!(0));
        assert!(!client_1.locked);
    }

    #[test]
    fn test_ignore_withdrawal_if_amount_is_none() {
        let records = vec![
            Ok(Transaction {
                kind: TransactionType::Deposit,
                client_id: 1,
                id: 1,
                amount: Some(dec!(12.34)),
            }),
            Ok(Transaction {
                kind: TransactionType::Withdrawal,
                client_id: 1,
                id: 2,
                amount: None,
            }),
        ];

        let clients = process_transactions(records).unwrap();
        let client_1 = clients.get(&1).unwrap();

        assert_eq!(client_1.available_funds, dec!(12.34));
        assert_eq!(client_1.total_funds, dec!(12.34));
        assert_eq!(client_1.held_funds, dec!(0));
        assert!(!client_1.locked);
    }
    // Withdraw funds before depositing ✅
    // Transaction ID repeated for withdraw ✅
    // Transaction ID repeated for deposit ✅
    // Check simple dispute ✅
    // Check dispute if no funds available ✅
    // Check simple resolve ✅
    // Check simple chargeback ✅
    // Check all type for locked account ✅
    // Ignore chargeback if not disputed ✅
    // Ignore resolve if not disputed ✅
    // Second dispute record is ignored ✅
    // Ignore dispute if Tx ID not present ✅
    // Ignore resolve if Tx ID not present ✅
    // Ignore dispute if Tx ID and client dont match ✅
    // Ignore resolve if Tx ID and client dont match ✅
    // Ignore if Dispute opened for withdrawal, not deposit ✅
    // Check if deposit or withdrawal sent without amount ✅
}
