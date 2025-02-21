use anyhow::{anyhow, Context, Result};
use futures::StreamExt;
use std::path::PathBuf;

use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::keygen::Keygen;
use round_based::async_runtime::AsyncProtocol;

use crate::{
    bs_client::join_computation,
    bs_signing::{do_sign, SigningConfig},
};

pub struct KeygenConfig {
    pub address: surf::Url,
    pub room: String,
    pub output: PathBuf,

    pub index: u16,
    pub threshold: u16,
    pub number_of_parties: u16,
}

#[derive(Debug)]
pub struct KeygenResult {
    pubkey: String,
    address: String,
    out_dir: PathBuf,
}

pub async fn do_keygen(config: KeygenConfig) -> Result<KeygenResult> {
    let mut output_file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&config.output)
        .await
        .context("cannot create output file")?;

    let (_i, incoming, outgoing) = join_computation(config.address, &config.room)
        .await
        .context("join computation")?;

    let incoming = incoming.fuse();
    tokio::pin!(incoming);
    tokio::pin!(outgoing);

    let keygen = Keygen::new(config.index, config.threshold, config.number_of_parties)?;
    let output = AsyncProtocol::new(keygen, incoming, outgoing)
        .run()
        .await
        .map_err(|e| anyhow!("protocol execution terminated with error: {}", e))?;

    let output = serde_json::to_vec_pretty(&output).context("serialize output")?;
    tokio::io::copy(&mut output.as_slice(), &mut output_file)
        .await
        .context("save output to file")?;

    let args = SigningConfig {
        room: "room".into(),
        address: "http://127.0.0.1:8000".parse()?,
        parties: vec![1, 2],
        local_share: config.output,
        data_to_sign: "boomersig go brrrr".into(),
        transaction: false,
    };

    let res = do_sign(args).await?;

    Ok(KeygenResult {
        pubkey: res.pubkey,
        address: res.address,
        out_dir: res.out_dir,
    })
}
