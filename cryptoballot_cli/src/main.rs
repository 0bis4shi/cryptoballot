use clap::{App, Arg, SubCommand};
use cryptoballot::*;
use lazy_static::lazy_static;
use sawtooth_sdk::signing::create_context;
use sawtooth_sdk::signing::CryptoFactory;
use sha2::Digest;
use sha2::Sha512;
use std::fs::File;
use std::io::prelude::*;
use tallystick::plurality::DefaultPluralityTally;

mod rest;
mod transaction;

fn main() {
    let matches = App::new("CryptoBallot CLI")
        .version("1.0")
        .author("Patrick Hayes <patrick.d.hayes@gmail.com>")
        .about("Interacts with a CryptoBallot server")
        .arg(
            Arg::with_name("uri")
                .help("Set the cryptoballot uri - can also be set with CRYPTOBALLOT_URI")
                .required(false),
        )
        .subcommand(
            SubCommand::with_name("generate")
                .about("Generate keypair")
                .arg(
                    Arg::with_name("algorithim")
                        .index(1)
                        .required(true) // TODO: allow stdin
                        .help("One of [ed25519, secp256k1, rsa]"),
                )
                .arg(
                    Arg::with_name("secret")
                        .long("secret")
                        .help("File location to write secret key")
                        .takes_value(true)
                        .required(true), // TODO: allow PEM format
                ),
        )
        .subcommand(
            SubCommand::with_name("sign")
                .about("Sign transaction")
                .arg(
                    Arg::with_name("INPUT")
                        .index(1)
                        .required(true) // TODO: allow stdin
                        .help("Transaction file in JSON or CBOR format"),
                )
                .arg(
                    Arg::with_name("secret")
                        .long("secret")
                        .takes_value(true)
                        .help("Secret ed25519 key file location in hex format")
                        .required(true), // TODO: allow environment variable && PEM format
                ),
        )
        .subcommand(
            SubCommand::with_name("get")
                .about("GET transaction")
                .arg(
                    Arg::with_name("id")
                        .index(1)
                        .required(true) // TODO: allow stdin
                        .help("Get Transaction by ID"),
                )
                .arg(
                    Arg::with_name("pretty")
                        .long("pretty")
                        .help("Pretty print JSON"),
                ),
        )
        .subcommand(
            SubCommand::with_name("post").about("Post transaction").arg(
                Arg::with_name("INPUT")
                    .index(1)
                    .required(true) // TODO: allow stdin
                    .help("Transaction file in JSON or CBOR format"),
            ),
        )
        .subcommand(
            SubCommand::with_name("tally").about("Tally Election").arg(
                Arg::with_name("election-id")
                    .index(1)
                    .required(true) // TODO: allow stdin
                    .help("Tally votes in an election to get a winner"),
            ),
        )
        .get_matches();

    // Gets a value for config if supplied by user, or defaults to "default.conf"
    let env_var = std::env::var("CRYPTOBALLOT_URI");
    let uri = match matches.value_of("uri") {
        Some(uri) => uri.to_string(),
        None => env_var.unwrap_or("http://localhost:8008".to_string()),
    };

    // Subcommands
    if let Some(matches) = matches.subcommand_matches("post") {
        command_post_transaction(matches, &uri);
    }
    if let Some(matches) = matches.subcommand_matches("generate") {
        command_generate(matches);
    }
    if let Some(matches) = matches.subcommand_matches("sign") {
        command_sign_transaction(matches);
    }
    if let Some(matches) = matches.subcommand_matches("get") {
        command_get_transaction(matches, &uri);
    }
    if let Some(matches) = matches.subcommand_matches("tally") {
        command_tally(matches, &uri);
    }
}

fn command_generate(matches: &clap::ArgMatches) {
    // Unwraps are OK, both these args are required
    let algo = matches.value_of("algorithim").unwrap();
    let secret_location = expand(matches.value_of("secret").unwrap());

    let (secret, public) = match algo {
        "ed25519" => {
            let (secret, public) = cryptoballot::generate_keypair();
            (
                hex::encode(secret.to_bytes()),
                hex::encode(public.to_bytes()),
            )
        }
        _ => todo!("secp256k1 and rsa not implemented"), // TODO
    };

    let mut file = File::create(&secret_location).unwrap_or_else(|e| {
        eprintln!(
            "cryptoballot generate: cannot create file {}: {}",
            &secret_location, e
        );
        std::process::exit(1);
    });

    file.write_all(secret.as_bytes()).unwrap_or_else(|e| {
        eprintln!(
            "cryptoballot post: unable to write secret to {}: {}",
            &secret_location, e
        );
        std::process::exit(1);
    });

    println!("public-key: {}", public);
}

fn command_post_transaction(matches: &clap::ArgMatches, uri: &str) {
    let filename = expand(matches.value_of("INPUT").unwrap());

    let file_bytes = match std::fs::read(&filename) {
        Ok(bytes) => bytes,
        Err(e) => {
            eprintln!("cryptoballot post: unable to read {}: {}, ", &filename, e);
            std::process::exit(1);
        }
    };

    let tx = cryptoballot::SignedTransaction::from_bytes(&file_bytes).unwrap_or_else(|e| {
        // Maybe it's an unsigned transaction?
        if cryptoballot::Transaction::from_bytes(&file_bytes).is_ok() {
            eprintln!(
                "cryptoballot post: {} is unsigned, use `cryptoballot sign` to sign it first",
                filename
            );
        } else {
            eprintln!("cryptoballot post: unable to read {}: {}, ", &filename, e);
        }

        std::process::exit(1);
    });

    // Generate the sawtooth signer
    // TODO: allow sawtooth signer to be passed in
    let context = create_context("secp256k1").expect("Error creating the right context");
    let private_key = context
        .new_random_private_key()
        .expect("Error generating a new Private Key");
    let crypto_factory = CryptoFactory::new(context.as_ref());
    let signer = crypto_factory.new_signer(private_key.as_ref());

    // Create sawtooth transaction
    let tx = transaction::create_tx(&signer, &tx);
    let bl = transaction::create_batch_list(&signer, &tx);
    rest::send_batch_list(bl, uri).unwrap_or_else(|e| {
        eprintln!("cryptoballot post: error sending transaction: {}, ", e);
        std::process::exit(1);
    });
}

fn command_sign_transaction(matches: &clap::ArgMatches) {
    // Unwraps OK - required args
    let filename = expand(matches.value_of("algorithim").unwrap());
    let secret_location = expand(matches.value_of("secret").unwrap());

    let file_bytes = match std::fs::read(&filename) {
        Ok(bytes) => bytes,
        Err(e) => {
            eprintln!("cryptoballot post: unable to read {}: {}, ", &filename, e);
            std::process::exit(1);
        }
    };

    // TODO: FINISH THIS
    let _tx = cryptoballot::Transaction::from_bytes(&file_bytes).unwrap_or_else(|e| {
        // Maybe it's already signed?
        if cryptoballot::SignedTransaction::from_bytes(&file_bytes).is_ok() {
            eprintln!("cryptoballot sign: {} is already signed", filename);
        } else {
            eprintln!("cryptoballot post: unable to read {}: {}, ", filename, e);
        }

        std::process::exit(1);
    });

    let _key = std::fs::read_to_string(secret_location);

    todo!("command sign transaction not finish"); // TODO
}

fn command_get_transaction(matches: &clap::ArgMatches, uri: &str) {
    // Unwraps OK - required args
    let id = matches.value_of("id").unwrap();
    let id = id.parse().unwrap();

    // TODO: remove unwrap
    let tx = rest::get_transaction(id, uri).unwrap();

    let json_tx = if matches.is_present("pretty") {
        serde_json::to_string_pretty(&tx).unwrap()
    } else {
        serde_json::to_string(&tx).unwrap()
    };

    println!("{}", json_tx);
}

fn command_tally(matches: &clap::ArgMatches, uri: &str) {
    // Unwraps OK - required args
    let election_id = matches.value_of("election-id").unwrap();
    let election_id = election_id.parse().unwrap();

    // TODO: remove these unwraps, use try_into();
    let election = rest::get_transaction(election_id, uri).unwrap();
    let election: Signed<ElectionTransaction> = election.into();

    let vote_txs =
        rest::get_multiple_transactions(election.id(), Some(TransactionType::Decryption), uri)
            .unwrap();

    // TODO: Use a real tally / ballot / contest system
    let mut tally = DefaultPluralityTally::new(1);

    for vote in vote_txs {
        // TODO: use try_into();
        let vote: Signed<DecryptionTransaction> = vote.into();

        let selection = std::str::from_utf8(&vote.decrypted_vote)
            .unwrap()
            .to_owned();
        tally.add(selection);
    }

    let winners = tally.winners().into_unranked();
    println!("The winner is {}", winners[0]);
}

// Utility Functions
// -----------------

lazy_static! {
    static ref CB_PREFIX: String = {
        let mut sha = Sha512::new();
        sha.input("cryptoballot");
        hex::encode(&sha.result()[..3])
    };
}

pub fn identifier_to_address(ident: cryptoballot::Identifier) -> String {
    let prefix: &str = CB_PREFIX.as_ref();
    format!("{}{}", prefix, ident.to_string())
}

pub fn identifier_to_address_prefix(
    election_id: cryptoballot::Identifier,
    tx_type: Option<TransactionType>,
) -> String {
    let prefix: &str = CB_PREFIX.as_ref();
    let election_id = hex::encode(election_id.election_id);

    match tx_type {
        Some(tx_type) => {
            let tx_type = hex::encode([tx_type as u8]);
            format!("{}{}{}", prefix, election_id, tx_type)
        }
        None => format!("{}{}", prefix, election_id),
    }
}

// Performs shell expansion on filenames (mostly to handle ~)
fn expand(filename: &str) -> String {
    shellexpand::full(filename)
        .unwrap_or_else(|e| {
            eprintln!("cryptoballot: error expanding {}: {}", filename, e);
            std::process::exit(1);
        })
        .into_owned()
}
