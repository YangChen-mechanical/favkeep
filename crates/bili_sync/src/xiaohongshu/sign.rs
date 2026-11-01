// SPDX-License-Identifier: MIT
// Port of Cloxl/xhshow b242a84f9c5a6a971bc4c90c63ad43345a4bf8a1; see xhshow-LICENSE.
use super::codec::rc4;
use anyhow::{Result, ensure};
use base64::{
    Engine,
    alphabet::Alphabet,
    engine::{GeneralPurpose, general_purpose::PAD},
};
use serde_json::json;

const ALPHABET: &str = "ZmserbBoHQtNP+wOcza/LpngG8yJq42KWYj0DSfdikx3VT16IlUAFM97hECvuRX5";
const X3: &str = "MfgqrsbcyzPQRStuvC7mn501HIJBo2DEFTKdeNOwxWXYZap89+/A4UVLhijkl63G";
const KEY: &str = "71a302257793271ddd273bcee3e4b98d9d7935e1da33f5765e2ea8afb6dc77a51a499d23b67c20660025860cbf13d4540d92497f58686c574e508f46e1956344f39139bf4faf22a3eef120b79258145b2feb5193b6478669961298e79bedca646e1a693a926154a5a7a1bd1cf0dedb742f917a747a1e388b234f2277516db7116035439730fa61e9822a0eca7bff72d8";

fn encode(bytes: &[u8], alphabet: &str) -> String {
    GeneralPurpose::new(&Alphabet::new(alphabet).expect("fixed alphabet"), PAD).encode(bytes)
}

pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

fn hash(bytes: &[u8]) -> Vec<u8> {
    let n = bytes.len() as u32;
    let [mut a, mut b, mut c, mut d] = [1831565813u32, 461845907, 2246822507, 3266489909];
    a ^= n;
    b ^= n << 8;
    c ^= n << 16;
    d ^= n << 24;
    for chunk in bytes.as_chunks::<8>().0 {
        let x = u32::from_le_bytes(chunk[..4].try_into().unwrap());
        let y = u32::from_le_bytes(chunk[4..].try_into().unwrap());
        a = (a.wrapping_add(x) ^ c).rotate_left(7);
        b = (x ^ b).wrapping_add(d).rotate_left(11);
        c = (c.wrapping_add(y) ^ a).rotate_left(13);
        d = (d ^ y).wrapping_add(b).rotate_left(17);
    }
    let t0 = a ^ n;
    let t1 = b ^ t0;
    let t2 = c.wrapping_add(t1);
    let t3 = d ^ t2;
    a = t0.rotate_left(9).wrapping_add(t2.rotate_left(17));
    b = t1.rotate_left(13) ^ t3.rotate_left(19);
    c = t2.rotate_left(17).wrapping_add(a);
    d = t3.rotate_left(19) ^ b;
    [a, b, c, d].into_iter().flat_map(u32::to_le_bytes).collect()
}

pub struct Inputs {
    pub timestamp: u64,
    pub page_load: u64,
    pub seed: u32,
    pub sequence: u32,
    pub window: u32,
}

pub fn sign(path: &str, body: Option<&str>, a1: &str, inputs: &Inputs) -> Result<String> {
    ensure!(
        a1.len() == 52 && a1.bytes().all(|b| b.is_ascii_alphanumeric()),
        "invalid guest identifier"
    );
    ensure!(path.starts_with("/api/") && !path.contains('#'), "invalid request path");
    let content = format!("{path}{}", body.unwrap_or(""));
    let digest = md5::compute(content.as_bytes());
    let path_digest = md5::compute(if body.is_some() {
        path.as_bytes()
    } else {
        content.as_bytes()
    });
    let seed_byte = inputs.seed as u8;
    let mut payload = vec![121, 104, 96, 41];
    payload.extend(inputs.seed.to_le_bytes());
    payload.extend(inputs.timestamp.to_le_bytes());
    payload.extend(inputs.page_load.to_le_bytes());
    payload.extend(inputs.sequence.to_le_bytes());
    payload.extend(inputs.window.to_le_bytes());
    payload.extend((content.len() as u32).to_le_bytes());
    payload.extend(digest.0[..8].iter().map(|b| b ^ seed_byte));
    payload.push(52);
    payload.extend(a1.as_bytes());
    payload.push(10);
    payload.extend(b"xhs-pc-web");
    let table = [115, 248, 83, 102, 103, 201, 181, 131, 99, 94, 4, 68, 250, 132, 21];
    let checks = [0, 1, 18, 1, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 0];
    payload.extend([1, seed_byte ^ table[0]]);
    payload.extend((1..15).map(|i| table[i] ^ checks[i]));
    let mut input = inputs.timestamp.to_le_bytes().to_vec();
    input.extend(path_digest.0);
    payload.extend([2, 97, 51, 16]);
    payload.extend(hash(&input).iter().map(|b| b ^ seed_byte));
    ensure!(payload.len() == 144, "signature payload length");
    for (byte, key) in payload.iter_mut().zip(hex::decode(KEY)?) {
        *byte ^= key;
    }
    let data = json!({"x0":"4.3.5","x1":"xhs-pc-web","x2":"Windows","x3":format!("mns0301_{}",encode(&payload,X3)),"x4":"object"});
    Ok(format!(
        "XYS_{}",
        encode(serde_json::to_string(&data)?.as_bytes(), ALPHABET)
    ))
}

pub fn common(a1: &str, timestamp: u64, activity: u8) -> Result<String> {
    ensure!(
        a1.len() == 52 && a1.bytes().all(|b| b.is_ascii_alphanumeric()),
        "invalid guest identifier"
    );
    let fp = json!({"x33":"0","x34":"0","x35":"0","x36":activity.to_string(),"x37":"0|0|0|0|0|0|0|0|0|1|0|0|0|0|0|0|0|0|1|0|0|0|0|0","x38":"0|0|1|0|1|0|0|0|0|0|1|0|1|0|1|0|0|0|0|0|0|0|0|0|0|0|0|0|0|0|0|0|0|0|0|0|0|0|0","x39":0,"x42":"3.4.4","x43":"742cc32c","x44":timestamp.to_string(),"x45":"__SEC_CAV__1-1-1-1-1|__SEC_WSA__|","x46":"false","x48":"","x49":"{list:[],type:}","x50":"","x51":"","x52":"","x82":"_0x17a2|_0x1954"});
    let cipher = rc4(serde_json::to_string(&fp)?.as_bytes(), b"xhswebmplfbt");
    // The reference converts RC4 bytes through Latin-1, URL encoding, then drops any leading safe characters.
    let mut utf8 = String::new();
    for byte in cipher {
        utf8.push(char::from(byte));
    }
    let start = utf8
        .char_indices()
        .find(|(_, c)| !c.is_ascii_alphanumeric() && !"!*'()~_-.".contains(*c))
        .map(|(i, _)| i)
        .unwrap_or(utf8.len());
    let b1 = encode(&utf8.as_bytes()[start..], ALPHABET);
    let checksum = (crc32(b1.as_bytes()) ^ 0xedb8_8320) as i32;
    let data = format!(
        r#"{{"s0":5,"s1":"","x0":"1","x1":"4.3.5","x2":"Windows","x3":"xhs-pc-web","x4":"4.86.0","x5":"{a1}","x6":"","x7":"","x8":"{b1}","x9":{checksum},"x10":0,"x11":"normal"}}"#
    );
    Ok(encode(data.as_bytes(), ALPHABET))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pinned_reference_vectors() {
        let vectors: serde_json::Value = serde_json::from_str(include_str!("xhs-sign-vectors.json")).unwrap();
        for v in vectors.as_array().unwrap() {
            let inputs = Inputs {
                timestamp: v["timestamp"].as_u64().unwrap(),
                page_load: v["page_load"].as_u64().unwrap(),
                seed: v["seed"].as_u64().unwrap() as u32,
                sequence: v["sequence"].as_u64().unwrap() as u32,
                window: v["window"].as_u64().unwrap() as u32,
            };
            let a1 = v["a1"].as_str().unwrap();
            assert_eq!(
                sign(v["path"].as_str().unwrap(), v["body"].as_str(), a1, &inputs).unwrap(),
                v["signature"]
            );
            assert_eq!(common(a1, inputs.timestamp, 7).unwrap(), v["common"]);
        }
        assert!(
            sign(
                "/api/fixture",
                None,
                "bad",
                &Inputs {
                    timestamp: 1,
                    page_load: 1,
                    seed: 0,
                    sequence: 0,
                    window: 0
                }
            )
            .is_err()
        );
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    }
}
