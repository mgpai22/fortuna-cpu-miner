use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::mpsc::Sender;
use std::thread::JoinHandle;
use std::time::Duration;
use std::{env, process, thread};

use anyhow::anyhow;
use async_std::net::TcpStream;
use async_std::{
    io::BufReader,
    prelude::*,
    sync::{Arc, Mutex},
    task,
};
use clap::{arg, command, value_parser};
use env_logger::Env;
use log::{debug, error, info, trace, warn};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Serialize, Deserialize, Debug, Clone)]
struct StratumMessage {
    id: Option<serde_json::Value>,
    method: Option<String>,
    params: Option<serde_json::Value>,
    result: Option<serde_json::Value>,
    error: Option<serde_json::Value>,
}

#[derive(Serialize, Debug)]
struct FoundAnswerResponse {
    job_id: String,
    extra_nonce: Vec<u8>,
    nonce: Vec<u8>,
    answer: Vec<u8>,
    difficulty: u128,
    zeroes: u8,
}

struct Client {
    receiver_incoming: async_channel::Receiver<String>,
    sender_outgoing: async_channel::Sender<String>,
}

impl Client {
    pub async fn new(
        pool_host: &str,
        pool_port: u16,
        stratum_sender: tokio::sync::broadcast::Sender<StratumMessage>,
    ) -> Result<
        (
            Arc<Mutex<Client>>,
            tokio::sync::broadcast::Receiver<StratumMessage>,
        ),
        anyhow::Error,
    > {
        let stream = match TcpStream::connect((pool_host, pool_port)).await {
            Ok(stream) => stream,
            Err(e) => {
                error!("Failed to connect to the pool: {}", e);
                return Err(anyhow!("Failed to connect to the pool: {}", e));
            }
        };

        let (sender_outgoing, receiver_outgoing) = async_channel::bounded::<String>(10);
        let (sender_incoming, receiver_incoming) = async_channel::bounded::<String>(10);
        let (sender_subscription_response, receiver_subscription_response) =
            tokio::sync::broadcast::channel::<StratumMessage>(1024);

        let stream_clone = stream.clone();
        task::spawn(async move {
            let arc_stream = Arc::new(stream);
            let reader = &*arc_stream;
            let mut messages = BufReader::new(reader).lines();
            while let Some(message) = messages.next().await {
                match message {
                    Ok(msg) => {
                        if let Err(e) = sender_incoming.send(msg).await {
                            warn!("Failed to send incoming message: {}", e);
                            break;
                        }
                    }
                    Err(e) => {
                        warn!("Failed to read message: {}", e);
                        break;
                    }
                }
            }
        });

        task::spawn(async move {
            let arc_stream = Arc::new(stream_clone);
            let mut writer = &*arc_stream;
            while let Ok(message) = receiver_outgoing.recv().await {
                if let Err(e) = writer.write_all(message.as_bytes()).await {
                    warn!("Failed to write message: {}", e);
                    break;
                }
            }
        });

        let client = Client {
            receiver_incoming,
            sender_outgoing,
        };

        let client = Arc::new(Mutex::new(client));

        let cloned = client.clone();
        let cloned_sender = sender_subscription_response.clone();
        let stratum_sender_clone = stratum_sender.clone();
        task::spawn(async move {
            loop {
                if let Some(mut locked_client) = cloned.try_lock() {
                    if let Ok(message) = locked_client.receiver_incoming.try_recv() {
                        locked_client
                            .parse_message(message, &cloned_sender, &stratum_sender_clone)
                            .await;
                    }
                }
                task::sleep(Duration::from_millis(100)).await;
            }
        });

        Ok((client, receiver_subscription_response))
    }

    async fn send_message(&self, msg: StratumMessage) -> Result<(), String> {
        match serde_json::to_string(&msg) {
            Ok(serialized_msg) => {
                let msg = format!("{}\n", serialized_msg);
                if let Err(e) = self.sender_outgoing.send(msg).await {
                    warn!("Failed to send outgoing message: {}", e);
                    return Err(format!("Failed to send outgoing message: {}", e));
                } else {
                    Ok(())
                }
            }
            Err(e) => {
                warn!("Failed to serialize message: {}", e);
                Err(format!("Failed to serialize message: {}", e))
            }
        }
    }

    async fn parse_message(
        &mut self,
        message: String,
        sender_subscription_response: &tokio::sync::broadcast::Sender<StratumMessage>,
        stratum_sender: &tokio::sync::broadcast::Sender<StratumMessage>,
    ) {
        let stratum_message: StratumMessage = match serde_json::from_str(&message) {
            Ok(msg) => msg,
            Err(e) => {
                warn!("Failed to parse message: {}", e);
                return;
            }
        };

        if let Some(error) = &stratum_message.error {
            if let Some(error_obj) = error.as_object() {
                if let Some(code) = error_obj.get("code").and_then(|c| c.as_i64()) {
                    if let Some(message) = error_obj.get("message").and_then(|m| m.as_str()) {
                        error!("Error: Code: {}, Message: {}", code, message);
                    }
                }
            }
        } else {
            if let Some(id) = &stratum_message.id {
                if id == &Value::from(1) {
                    if let Some(result) = &stratum_message.result {
                        if let Err(e) = sender_subscription_response.send(StratumMessage {
                            id: stratum_message.id.clone(),
                            method: None,
                            params: None,
                            error: None,
                            result: Some(result.clone()),
                        }) {
                            warn!("Failed to send subscription response: {}", e);
                        }
                    }
                }
            }

            if let Some(id) = &stratum_message.id {
                match id {
                    Value::Number(num) if num.as_u64() == Some(2) => {
                        if let Some(result) = &stratum_message.result {
                            match result {
                                Value::Bool(true) => {
                                    info!("Connected to Pool: Authorized worker");
                                }
                                _ => {
                                    info!("Pool Connection Failed: Unauthorized");
                                    process::exit(1);
                                }
                            }
                        } else {
                            info!("Pool Connection Failed: Unauthorized");
                            process::exit(1);
                        }
                    }
                    Value::Number(num) if num.as_u64() == Some(4) => {
                        if let Some(result) = &stratum_message.result {
                            match result {
                                Value::Bool(true) => {
                                    info!("Share accepted");
                                }
                                _ => {
                                    info!("Share Failed!!!");
                                }
                            }
                        } else {
                            warn!("Share Failed!!!");
                        }
                    }
                    _ => {}
                }
            }

            if let Some(result) = &stratum_message.result {
                if let Some(error) = result.as_object().and_then(|obj| obj.get("error")) {
                    error!("Error received: {:?}", error);
                }
            }

            if let Err(e) = stratum_sender.send(stratum_message) {
                warn!("Failed to send stratum message: {}", e);
            }
        }
    }
}

async fn connect(
    pool_host: &str,
    pool_port: u16,
    username: &str,
    password: &str,
    stratum_sender: tokio::sync::broadcast::Sender<StratumMessage>,
) -> Result<
    (
        Arc<Mutex<Client>>,
        tokio::sync::broadcast::Receiver<StratumMessage>,
        String,
        usize,
    ),
    anyhow::Error,
> {
    loop {
        match Client::new(pool_host, pool_port, stratum_sender.clone()).await {
            Ok((client, mut receiver_subscription_response)) => {
                let subscribe_msg = StratumMessage {
                    id: Some(serde_json::Value::from(1)),
                    method: Some("mining.subscribe".to_string()),
                    params: Some(serde_json::json!([])),
                    result: None,
                    error: None,
                };
                let _ = client.lock().await.send_message(subscribe_msg).await;

                let subscription_response: Result<StratumMessage, anyhow::Error> =
                    match tokio::time::timeout(Duration::from_secs(10), async {
                        loop {
                            if let Ok(msg) = receiver_subscription_response.try_recv() {
                                return Ok::<StratumMessage, anyhow::Error>(msg);
                            }
                            tokio::time::sleep(Duration::from_millis(100)).await;
                        }
                    })
                    .await
                    {
                        Ok(Ok(msg)) => Ok(msg),
                        Ok(Err(e)) => Err(anyhow!("Failed to receive message: {}", e)),
                        Err(e) => {
                            warn!("Subscription response timeout: {}", e);
                            Err(anyhow!("Subscription response timeout: {}", e))
                        }
                    };

                let subscription_response = subscription_response?;

                let mut extra_nonce1 = String::new();
                let mut extra_nonce2_bytes_size = 0;

                if let Some(result) = subscription_response.result.clone() {
                    if let Some(array) = result.as_array() {
                        if array.len() >= 3 {
                            if let Some(extraNonce1) = array.get(1).and_then(|v| v.as_str()) {
                                extra_nonce1 = extraNonce1.to_string();
                            } else {
                                error!("extraNonce1 not found or not a string");
                            }

                            if let Some(extra_nonce2_size) = array.get(2).and_then(|v| v.as_u64()) {
                                let extraNonce2Size = extra_nonce2_size as usize;
                                extra_nonce2_bytes_size = extraNonce2Size;
                            } else {
                                error!("extraNonce2 size not found or not a number");
                            }
                        } else {
                            error!("Result array does not contain enough elements");
                        }
                    } else {
                        error!("Result is not an array");
                    }
                } else {
                    error!("Subscription response does not contain result");
                }

                let authorize_msg = StratumMessage {
                    id: Some(serde_json::Value::from(2)),
                    method: Some("mining.authorize".to_string()),
                    params: Some(serde_json::json!([username, password])),
                    result: None,
                    error: None,
                };
                let _ = client.lock().await.send_message(authorize_msg).await;

                return Ok((
                    client,
                    receiver_subscription_response,
                    extra_nonce1,
                    extra_nonce2_bytes_size,
                ));
            }
            Err(e) => {
                error!("Failed to create client: {}. Retrying in 10 seconds...", e);
                tokio::time::sleep(Duration::from_secs(10)).await;
            }
        }
    }
}

#[tokio::main]
async fn main() {
    let env = Env::default()
        .filter_or("MY_LOG_LEVEL", "debug")
        .write_style_or("MY_LOG_STYLE", "always");

    env_logger::init_from_env(env);

    let matches = command!()
        .arg(
            arg!(--pool <HOSTPORT> "The pool host and port")
                .required(true)
                .value_parser(value_parser!(String)),
        )
        .arg(
            arg!(--threads <NUM> "The number of threads to use")
                .required(false)
                .value_parser(value_parser!(usize)),
        )
        .arg(
            arg!(--user <USERNAME> "The username for authentication")
                .required(true)
                .value_parser(value_parser!(String)),
        )
        .arg(
            arg!(--pass <PASSWORD> "The password for authentication")
                .required(false)
                .value_parser(value_parser!(String)),
        )
        .arg(
            arg!(--difficulty <DIFFICULTY> "The custom difficulty to use")
                .required(false)
                .value_parser(value_parser!(u8)),
        )
        .get_matches();

    let pool_host_port = matches.get_one::<String>("pool").unwrap().clone();
    let (pool_host, pool_port_str) = pool_host_port.split_once(':').unwrap();
    let pool_host = pool_host.to_string();
    let pool_port: u16 = pool_port_str.parse().unwrap();

    let num_threads = matches
        .get_one::<usize>("threads")
        .map(|&v| v)
        .unwrap_or_else(|| {
            let default_threads = num_cpus::get() / 2;
            warn!(
                "Number of threads not specified. Using default: {}",
                default_threads
            );
            default_threads
        });

    let user_name = matches.get_one::<String>("user").unwrap().clone();

    let password = matches
        .get_one::<String>("pass")
        .map(|v| v.to_string())
        .unwrap_or_else(|| {
            warn!("No password provided. Continuing without password.");
            "".to_string()
        });

    let miner_zero_difficulty = matches.get_one::<u8>("difficulty").map(|&v| v).unwrap_or(5);

    info!("Connecting to Pool: {}", pool_host_port);
    info!("Worker: {}", user_name);
    info!("Miner default difficulty: {}", miner_zero_difficulty);
    info!("Override difficulty: {}", matches.contains_id("difficulty"));

    if matches.contains_id("threads") {
        info!("Using: {} Number of Threads", num_threads);
    }

    let (stratum_sender, stratum_receiver) =
        tokio::sync::broadcast::channel::<StratumMessage>(1024);
    let (
        mut client,
        mut receiver_subscription_response,
        mut extra_nonce_1,
        mut extra_nonce_2_bytes_size,
    ) = match connect(
        &pool_host,
        pool_port,
        user_name.as_str(),
        password.as_str(),
        stratum_sender.clone(),
    )
    .await
    {
        Ok(result) => result,
        Err(e) => {
            error!("Failed to connect: {}", e);
            return;
        }
    };

    let client_clone = client.clone();

    let should_terminate = Arc::new(AtomicBool::new(false));
    let total_hashes = Arc::new(AtomicUsize::new(0));
    let total_hashes_clone = Arc::clone(&total_hashes);

    thread::spawn(move || {
        let mut previous_hashes = 0;
        loop {
            thread::sleep(Duration::from_secs(30));
            let current_hashes = total_hashes_clone.load(Ordering::Relaxed);
            let new_hashes = current_hashes - previous_hashes;
            previous_hashes = current_hashes;
            let hash_rate = new_hashes as f32 / 30f32;
            info!("Average Hash Rate: {:.2} hashes/sec", hash_rate);
        }
    });

    let (tx, rx) = mpsc::channel();
    let should_terminate_clone = should_terminate.clone();
    let total_hashes_clone = total_hashes.clone();

    let user_name_clone = user_name.clone();

    tokio::spawn(async move {
        loop {
            match operation(
                num_threads,
                matches.contains_id("difficulty"),
                miner_zero_difficulty,
                stratum_sender.clone(),
                should_terminate_clone.clone(),
                total_hashes_clone.clone(),
                tx.clone(),
                extra_nonce_1.clone(),
                extra_nonce_2_bytes_size.clone(),
            )
            .await
            {
                Ok(_) => {
                    // Operation completed successfully
                }
                Err(e) => {
                    error!("Error in operation: {}", e);
                    warn!("Sleeping for 10 seconds before reconnecting...");
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    // Attempt to reconnect
                    match connect(
                        &pool_host,
                        pool_port,
                        user_name.as_str(),
                        password.as_str(),
                        stratum_sender.clone(),
                    )
                    .await
                    {
                        Ok((
                            new_client,
                            new_receiver_subscription_response,
                            new_extra_nonce_1,
                            new_extra_nonce_2_bytes_size,
                        )) => {
                            client = new_client;
                            receiver_subscription_response = new_receiver_subscription_response;
                            extra_nonce_1 = new_extra_nonce_1;
                            extra_nonce_2_bytes_size = new_extra_nonce_2_bytes_size;
                        }
                        Err(e) => {
                            error!("Failed to reconnect: {}", e);
                        }
                    }
                }
            }
        }
    });

    loop {
        match rx.try_recv() {
            Ok(answer) => {
                info!("Found a solution!: {:?}", &answer.zeroes);
                debug!("Hash: {}", hex::encode(&answer.answer));
                debug!("Nonce: {}", hex::encode(&answer.nonce));
                debug!(
                    "Full Nonce: {}",
                    hex::encode(&answer.extra_nonce) + &*hex::encode(&answer.nonce)
                );
                let submit_msg = StratumMessage {
                    id: Some(serde_json::Value::from(3)),
                    method: Some("mining.submit".to_string()),
                    params: Some(serde_json::json!([
                        user_name_clone.as_str(),
                        &answer.job_id,
                        hex::encode(&answer.nonce)
                    ])),
                    result: None,
                    error: None,
                };
                let client = client_clone.clone();
                tokio::spawn(async move {
                    if let Err(_) = client.lock().await.send_message(submit_msg).await {
                        error!("Lost stratum connection, exiting...");
                        process::exit(1);
                    }
                });
            }
            Err(mpsc::TryRecvError::Empty) => {
                // No worker has found a solution yet. Continue polling or doing other tasks.
            }
            Err(mpsc::TryRecvError::Disconnected) => panic!("Channel disconnected"),
        }
    }
}

async fn operation(
    num_threads: usize,
    override_difficulty: bool,
    default_difficulty: u8,
    stratum_sender: tokio::sync::broadcast::Sender<StratumMessage>,
    should_terminate: Arc<AtomicBool>,
    total_hashes: Arc<AtomicUsize>,
    tx: Sender<FoundAnswerResponse>,
    extra_nonce_1: String,
    extra_nonce_2_bytes_size: usize,
) -> Result<(), anyhow::Error> {
    let mut job_id: Option<String> = None;
    let mut block_data: Option<String> = None;
    let mut refresh: bool = false;

    let mut handles: Vec<JoinHandle<()>> = vec![];

    let mut zero_difficulty = default_difficulty;
    let mut network_leading_zeros = default_difficulty;
    let mut network_difficulty = default_difficulty;

    let mut stratum_receiver = stratum_sender.subscribe();
    while let Ok(stratum_message) = stratum_receiver.recv().await {
        if let Some(method) = &stratum_message.method {
            if method == "mining.notify" {
                let params = stratum_message.params.clone().unwrap();
                debug!("{}", params);
                if let Some(job_id_str) = params.get(0).and_then(|v| v.as_str()) {
                    job_id = Some(job_id_str.to_string());
                }
                if let Some(parsed_block_data) = params.get(1).and_then(|v| v.as_str()) {
                    block_data = Some(parsed_block_data.to_string());
                }
                if let Some(network_leading_zeros_parsed) = params.get(2).and_then(|v| v.as_u64()) {
                    network_leading_zeros = network_leading_zeros_parsed as u8;
                }
                if let Some(network_difficulty_parsed) = params.get(3).and_then(|v| v.as_u64()) {
                    network_difficulty = network_difficulty_parsed as u8;
                }
                if let Some(parsed_refresh) = params.get(4).and_then(|v| v.as_bool()) {
                    refresh = parsed_refresh;
                }
            } else if method == "mining.set_difficulty" {
                let params = stratum_message.params.clone().unwrap();
                if let Some(zero_diff) = params.get(0).and_then(|v| v.as_u64()) {
                    if !override_difficulty {
                        info!("Setting Difficulty: {}", zero_diff);
                        zero_difficulty = zero_diff as u8
                    } else {
                        info!("Overriding Difficulty: {}", default_difficulty);
                    }
                } else {
                    error!(
                        "Invalid params format for mining.set_difficulty, using default difficulty"
                    );
                }
            }
        }

        if let (Some(job_id), Some(block_data)) = (job_id.clone(), block_data.clone()) {
            if refresh {
                should_terminate.store(true, Ordering::Relaxed);
                while let Some(handle) = handles.pop() {
                    handle.join().unwrap();
                }
                should_terminate.store(false, Ordering::Relaxed);

                for _ in 0..num_threads {
                    let should_terminate = Arc::clone(&should_terminate);
                    let tx = tx.clone();
                    let total_hashes_clone = total_hashes.clone();
                    let job_id_clone = job_id.clone();
                    let block_data_clone = block_data.clone();
                    let extra_nonce_1_clone = extra_nonce_1.clone();
                    let handle = thread::spawn(move || {
                        worker(
                            &job_id_clone,
                            &block_data_clone,
                            should_terminate,
                            tx,
                            total_hashes_clone,
                            network_difficulty,
                            zero_difficulty,
                            extra_nonce_1_clone,
                            extra_nonce_2_bytes_size,
                        );
                    });
                    handles.push(handle);
                }

                refresh = false;
            }
        }

        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    Ok(())
}

fn worker(
    job_id: &str,
    json: &str,
    should_terminate: Arc<AtomicBool>,
    tx: mpsc::Sender<FoundAnswerResponse>,
    total_hashes: Arc<AtomicUsize>,
    expected_diff: u8,
    expected_zeroes: u8,
    extra_nonce_1: String,
    extra_nonce_2_bytes_size: usize,
) {
    let mut bytes = hex::decode(json).unwrap();

    let mut iters = 0u32;
    let mut rng = rand::thread_rng();

    // Convert extra_nonce_1 to bytes
    let extra_nonce_1_bytes = hex::decode(extra_nonce_1).unwrap();
    let extra_nonce_1_length = extra_nonce_1_bytes.len();

    // Ensure the combined length of extra_nonce_1 and generated_nonce is 16 bytes
    if extra_nonce_1_length + extra_nonce_2_bytes_size != 16 {
        error!("Combined nonce length must be 16 bytes");
        panic!("Combined nonce length must be 16 bytes");
    }

    // Embed extra_nonce_1 into bytes
    bytes[4..4 + extra_nonce_1_length].copy_from_slice(&extra_nonce_1_bytes);

    // Generate initial generated_nonce with a length of extra_nonce_2_bytes_size
    let mut generated_nonce = vec![0u8; extra_nonce_2_bytes_size];
    rng.fill_bytes(&mut generated_nonce);

    while !should_terminate.load(Ordering::Relaxed) {
        // Copy the generated_nonce into the appropriate position in bytes
        bytes[4 + extra_nonce_1_length..4 + extra_nonce_1_length + extra_nonce_2_bytes_size]
            .copy_from_slice(&generated_nonce);

        let hashed_data = sha256_digest_as_bytes(&bytes);
        let hashed_hash = sha256_digest_as_bytes(&hashed_data);
        let (zeroes, difficulty) = get_difficulty(&hashed_hash);
        total_hashes.fetch_add(1, Ordering::Relaxed);
        iters = iters + 1u32;
        if zeroes > expected_zeroes
            || (zeroes == expected_zeroes && difficulty < expected_diff as u128)
        {
            tx.send(FoundAnswerResponse {
                job_id: job_id.to_string(),
                extra_nonce: extra_nonce_1_bytes.clone(),
                nonce: bytes
                    [4 + extra_nonce_1_length..4 + extra_nonce_1_length + extra_nonce_2_bytes_size]
                    .to_vec(),
                answer: hashed_hash.to_vec(),
                difficulty,
                zeroes,
            })
            .unwrap();
        }

        // Increment only the generated_nonce part of the nonce
        increment_u8_array(&mut generated_nonce);
    }
}

fn sha256_digest_as_bytes(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    let arr: [u8; 32] = result.into();
    arr
}

pub fn get_difficulty(hash: &[u8]) -> (u8, u128) {
    if hash.len() != 32 {
        error!("Expected a hash of length 32, but got {}", hash.len());
        panic!("Expected a hash of length 32, but got {}", hash.len());
    }

    let mut leading_zeros: u8 = 0;
    let mut difficulty_number = 0;

    for (indx, &chr) in hash.iter().enumerate() {
        if chr == 0 {
            leading_zeros += 2;
        } else {
            leading_zeros += (chr & 0xF0 == 0) as u8;
            if chr & 0x0F == chr {
                difficulty_number += (chr as u128) * 4096;
                difficulty_number += (hash[indx + 1] as u128) * 16;
                difficulty_number += (hash[indx + 2] as u128) / 16;
            } else {
                difficulty_number += (chr as u128) * 256;
                difficulty_number += hash[indx + 1] as u128;
            }
            break;
        }
    }

    (leading_zeros, difficulty_number)
}

pub fn increment_u8_array(x: &mut [u8]) {
    for byte in x.iter_mut() {
        if *byte == 255 {
            *byte = 0;
        } else {
            *byte += 1;
            break;
        }
    }
}
