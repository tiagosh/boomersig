use std::path::PathBuf;

use anyhow::Ok;
use anyhow::{anyhow, Context, Result};
use bitcoin::consensus::encode::{deserialize, serialize_hex};
use bitcoin::psbt::PartiallySignedTransaction;
use bitcoin::script::PushBytesBuf;
use bitcoin::sighash;
use bitcoin::ScriptBuf;
use bitcoin::Transaction;
use futures::{SinkExt, StreamExt, TryStreamExt};
use hex::FromHex;
use std::str::FromStr;
use structopt::StructOpt;

use curv::arithmetic::Converter;
use curv::BigInt;

use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::state_machine::sign::{
    OfflineStage, SignManual,
};
use round_based::async_runtime::AsyncProtocol;
use round_based::Msg;

use crate::bs_client::join_computation;

use openssl::bn::BigNum;

use secp256k1::{Message, RecoverableSignature, RecoveryId, Secp256k1};

pub struct SigningConfig {
    pub address: surf::Url,
    pub room: String,
    pub local_share: PathBuf,
    pub parties: Vec<u16>,
    pub data_to_sign: String,
    pub transaction: bool,
}

#[derive(Debug)]
pub struct SigningResult {
    pub pubkey: String,
    pub address: String,
    pub out_dir: PathBuf,
    pub signined_tx: Option<String>,
}

pub async fn do_sign(args: SigningConfig) -> Result<SigningResult> {
    let local_share = tokio::fs::read(args.local_share.clone())
        .await
        .context("cannot read local share")?;

    let local_share = serde_json::from_slice(&local_share).context("parse local share")?;
    let number_of_parties = args.parties.len();

    let (i, incoming, outgoing) =
        join_computation(args.address.clone(), &format!("{}-offline", args.room))
            .await
            .context("join offline computation")?;

    let incoming = incoming.fuse();
    tokio::pin!(incoming);
    tokio::pin!(outgoing);

    let signing = OfflineStage::new(i, args.parties, local_share)?;
    let completed_offline_stage = AsyncProtocol::new(signing, incoming, outgoing)
        .run()
        .await
        .map_err(|e| anyhow!("protocol execution terminated with error: {}", e))?;

    let (_i, incoming, outgoing) = join_computation(args.address, &format!("{}-online", args.room))
        .await
        .context("join online computation")?;

    tokio::pin!(incoming);
    tokio::pin!(outgoing);

    let mut tx: PartiallySignedTransaction;

    let data = match args.transaction {
        true => {
            tx = PartiallySignedTransaction::from_str(args.data_to_sign.as_str()).unwrap();
            let mut sighash_cache = sighash::SighashCache::new(tx.clone().extract_tx());
            let sighash_ecdsa = tx.sighash_ecdsa(0, &mut sighash_cache).unwrap();
            hex::decode(sighash_ecdsa.0.to_string()).unwrap()
        }
        false => {
            let _tx: Transaction =
                deserialize(&Vec::from_hex("010000000000ffffffff").unwrap()).unwrap();
            tx = PartiallySignedTransaction::from_unsigned_tx(_tx).unwrap();
            args.data_to_sign.as_bytes().to_vec()
        }
    };

    let (signing, partial_signature) =
        SignManual::new(BigInt::from_bytes(&data), completed_offline_stage)?;

    outgoing
        .send(Msg {
            sender: i,
            receiver: None,
            body: partial_signature,
        })
        .await?;

    let partial_signatures: Vec<_> = incoming
        .take(number_of_parties - 1)
        .map_ok(|msg| msg.body)
        .try_collect()
        .await?;

    let signature = signing
        .complete(&partial_signatures)
        .context("online stage failed")?;

    let r_bn = BigNum::from_slice(&signature.r.to_bytes())?;
    let s_bn = BigNum::from_slice(&signature.s.to_bytes())?;

    let secp = Secp256k1::new();
    let recid = RecoveryId::from_i32(signature.recid as i32).unwrap();
    let sig =
        RecoverableSignature::from_compact(&secp, &[r_bn.to_vec(), s_bn.to_vec()].concat(), recid)
            .unwrap();
    // println!(
    //     "sig: {:?}",
    //     hex::encode(sig.to_standard(&secp).serialize_der(&secp))
    // );
    let msg = Message::from_slice(&data)?;
    let public_key = secp.recover(&msg, &sig)?;
    let public_key_hex = hex::encode(public_key.serialize_vec(&secp, false));
    // println!(
    //     "pubkey {:?}",
    //     hex::encode(public_key.serialize_vec(&secp, false))
    // );
    // let signature = serde_json::to_string(&signature).context("serialize signature")?;
    // println!("sig {}", signature);

    if args.transaction {
        let mut script_sig = ScriptBuf::new();
        let mut v = PushBytesBuf::new();
        let mut sig = sig.to_standard(&secp).serialize_der(&secp);
        sig.push(1);
        v.extend_from_slice(&sig).unwrap();
        script_sig.push_slice(&v);

        let mut v = PushBytesBuf::new();
        v.extend_from_slice(&public_key.serialize_vec(&secp, false))
            .unwrap();
        script_sig.push_slice(&v);
        tx.inputs[0].final_script_sig = Some(script_sig);

        let tx = tx.extract_tx();

        let public_key =
            bitcoin::PublicKey::from_slice(&hex::decode(&public_key_hex).unwrap()).unwrap();
        let address = bitcoin::Address::p2pkh(&public_key, bitcoin::Network::Signet);

        return Ok(SigningResult {
            pubkey: public_key_hex,
            address: address.to_string(),
            out_dir: args.local_share,
            signined_tx: Some(serialize_hex(&tx)),
        });
    }

    let public_key =
        bitcoin::PublicKey::from_slice(&hex::decode(&public_key_hex).unwrap()).unwrap();
    let address = bitcoin::Address::p2pkh(&public_key, bitcoin::Network::Signet);

    Ok(SigningResult {
        pubkey: public_key_hex,
        address: address.to_string(),
        out_dir: args.local_share,
        signined_tx: None,
    })
}
