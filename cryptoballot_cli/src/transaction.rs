use protobuf::Message;
use protobuf::RepeatedField;
use rand::{thread_rng, Rng};
use sawtooth_sdk::messages::batch::Batch;
use sawtooth_sdk::messages::batch::BatchHeader;
use sawtooth_sdk::messages::batch::BatchList;
use sawtooth_sdk::messages::transaction::Transaction;
use sawtooth_sdk::messages::transaction::TransactionHeader;
use sawtooth_sdk::signing::Signer;
use sha2::Digest;
use sha2::Sha512;

use super::*;

pub fn create_tx(signer: &Signer, tx: &cryptoballot::SignedTransaction) -> Transaction {
    let inputs = tx.inputs();
    let outputs = vec![tx.id()];

    let payload_bytes = tx.as_bytes();
    let tx_header_bytes = create_header(signer, &payload_bytes, inputs, outputs);

    let signature = signer
        .sign(&tx_header_bytes)
        .expect("Error signing the transaction header");

    let mut tx = Transaction::new();
    tx.set_header(tx_header_bytes.to_vec());
    tx.set_header_signature(signature);
    tx.set_payload(payload_bytes);

    tx
}

fn create_header(
    signer: &Signer,
    payload_bytes: &[u8],
    inputs: Vec<cryptoballot::Identifier>,
    outputs: Vec<cryptoballot::Identifier>,
) -> Vec<u8> {
    let mut txn_header = TransactionHeader::new();
    txn_header.set_family_name(String::from("cryptoballot"));
    txn_header.set_family_version(String::from("1.0"));

    // Translate the identifiers to addresses
    let mut input_addresses = Vec::with_capacity(inputs.len());
    for ident in inputs {
        input_addresses.push(identifier_to_address(ident));
    }
    let mut output_addresses = Vec::with_capacity(outputs.len());
    for ident in outputs {
        output_addresses.push(identifier_to_address(ident));
    }

    // Generate a random 128 bit number to use as a nonce
    let mut nonce = [0u8; 16];
    thread_rng()
        .try_fill(&mut nonce[..])
        .expect("Error generating random nonce");
    txn_header.set_nonce(to_hex_string(&nonce.to_vec()));

    txn_header.set_inputs(RepeatedField::from_vec(input_addresses));
    txn_header.set_outputs(RepeatedField::from_vec(output_addresses));
    txn_header.set_signer_public_key(
        signer
            .get_public_key()
            .expect("Error retrieving Public Key")
            .as_hex(),
    );
    txn_header.set_batcher_public_key(
        signer
            .get_public_key()
            .expect("Error retrieving Public Key")
            .as_hex(),
    );

    txn_header.set_payload_sha512(to_hex_string(&Sha512::digest(&payload_bytes).to_vec()));

    let txn_header_bytes = txn_header
        .write_to_bytes()
        .expect("Error converting transaction header to bytes");

    txn_header_bytes
}

// To properly format the Sha512 String
pub fn to_hex_string(bytes: &Vec<u8>) -> String {
    let strs: Vec<String> = bytes.iter().map(|b| format!("{:02x}", b)).collect();
    strs.join("")
}

pub fn create_batch_header(signer: &Signer, tx: &Transaction) -> Vec<u8> {
    let mut batch_header = BatchHeader::new();

    batch_header.set_signer_public_key(
        signer
            .get_public_key()
            .expect("Error retrieving Public Key")
            .as_hex(),
    );

    let transaction_ids = vec![tx.clone()]
        .iter()
        .map(|trans| String::from(trans.get_header_signature()))
        .collect();

    batch_header.set_transaction_ids(RepeatedField::from_vec(transaction_ids));

    let batch_header_bytes = batch_header
        .write_to_bytes()
        .expect("Error converting batch header to bytes");

    batch_header_bytes
}

fn create_batch(signer: &Signer, tx: &Transaction) -> Batch {
    let batch_header_bytes = create_batch_header(signer, tx);

    let signature = signer
        .sign(&batch_header_bytes)
        .expect("Error signing the batch header");

    let mut batch = Batch::new();

    batch.set_header(batch_header_bytes);
    batch.set_header_signature(signature);
    batch.set_transactions(RepeatedField::from_vec(vec![tx.clone()]));

    batch
}

pub fn create_batch_list(signer: &Signer, tx: &Transaction) -> Vec<u8> {
    let batch = create_batch(signer, tx);

    let mut batch_list = BatchList::new();
    batch_list.set_batches(RepeatedField::from_vec(vec![batch]));
    let batch_list_bytes = batch_list
        .write_to_bytes()
        .expect("Error converting batch list to bytes");

    batch_list_bytes
}

#[cfg(test)]
mod test {

    use super::*;

    #[test]
    fn test_address_translation() {
        let identifier = cryptoballot::Identifier::new_for_election();
        let address = identifier_to_address(identifier);

        assert!(address.len() == 70);
    }
}
