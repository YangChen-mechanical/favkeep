// SPDX-License-Identifier: MIT
// Passport field encodings follow cv-cat/catbus bcc72f81549b94090b340fb87f29927529056e49; see catbus-LICENSE.
use anyhow::{Context, Result};
use base64::{
    Engine,
    alphabet::Alphabet,
    engine::{GeneralPurpose, general_purpose::PAD},
};
use sm3::{Digest, Sm3};
use std::collections::BTreeMap;

pub const UA: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/155.0.0.0 Safari/537.36";
pub const CH_UA: &str = "\"Google Chrome\";v=\"155\", \"Chromium\";v=\"155\", \"Not(A:Brand\";v=\"24\"";

// Public SDK protocol constant, not an account credential.
const APP_KEY: &str = "163e7ce78d58971a41f5b969996d85c2";

pub fn encrypt(text: &str) -> String {
    text.as_bytes().iter().map(|b| format!("{:x}", b ^ 5)).collect()
}

pub fn sdk_ts(now_ms: i64) -> Result<String> {
    let day = chrono::DateTime::from_timestamp_millis(now_ms)
        .context("invalid SDK timestamp")?
        .date_naive();
    Ok(day.and_hms_opt(12, 0, 0).unwrap().and_utc().timestamp().to_string())
}

fn hmac(key: &[u8], data: &[u8]) -> Vec<u8> {
    ring::hmac::sign(&ring::hmac::Key::new(ring::hmac::HMAC_SHA256, key), data)
        .as_ref()
        .to_vec()
}

pub fn aid_sign(path: &str, timestamp: &str) -> String {
    let prk = hmac(timestamp.as_bytes(), APP_KEY.as_bytes());
    let okm = hmac(&prk, &[1]);
    hex::encode(hmac(&okm, format!("aid=6383&path={path}&ts={timestamp}").as_bytes()))
}

pub fn seal(query: &[(&str, String)], body: &[(&str, String)]) -> (String, String) {
    let query: BTreeMap<_, _> = query
        .iter()
        .filter(|(k, _)| !["sign", "qs", "msToken", "a_bogus"].contains(k))
        .map(|(k, v)| (*k, v.as_str()))
        .collect();
    let keys: Vec<_> = query.keys().take(10).copied().collect();
    let q = keys
        .iter()
        .map(|k| format!("{k}={}", query[k]))
        .collect::<Vec<_>>()
        .join("&");
    let body: BTreeMap<_, _> = body.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let b = body
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&");
    let preimage = format!("{q}&{b}&app_key={APP_KEY}");
    (
        hex::encode(ring::digest::digest(&ring::digest::SHA256, preimage.as_bytes())),
        encrypt(&keys.join(",")),
    )
}

pub fn query(pairs: &[(&str, String)]) -> String {
    pairs
        .iter()
        .map(|(key, value)| {
            let value: String = value
                .bytes()
                .map(|b| {
                    if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                        char::from(b).to_string()
                    } else {
                        format!("%{b:02X}")
                    }
                })
                .collect();
            format!("{key}={value}")
        })
        .collect::<Vec<_>>()
        .join("&")
}

fn encode(bytes: &[u8], alphabet: &str) -> String {
    GeneralPurpose::new(&Alphabet::new(alphabet).expect("fixed alphabet"), PAD).encode(bytes)
}

pub fn web_sign(query: &str, uifid: &str, seconds: i64) -> String {
    let canon = query
        .replace("%21", "!")
        .replace("%27", "'")
        .replace("%28", "(")
        .replace("%29", ")")
        .replace("%2A", "*");
    let signed = format!("{canon}&uifid={uifid}&timestamp={seconds}");
    let digest = md5::compute(format!("{uifid}_{seconds}_A96D855A08C0A9707F8BEF0D9A527E4E_{signed}"));
    format!("{signed}&x-secsdk-web-signature={digest:x}")
}

pub fn str_data(plaintext: &str, nonce: u8) -> String {
    let mut bytes = vec![0x41, nonce];
    bytes.extend(super::codec::rc4(plaintext.as_bytes(), &[nonce]));
    encode(
        &bytes,
        "Dkdpgh4ZKsQB80/Mfvw36XI1R25+WUAlEi7NLboqYTOPuzmFjJnryx9HVGcaStCe",
    )
}

fn rc4_variant(bytes: &[u8], key: &[u8]) -> Vec<u8> {
    let mut state = std::array::from_fn::<_, 256, _>(|i| 255 - i);
    let mut j = 0;
    for i in 0..256 {
        j = (j * state[i] + j + usize::from(key[i % key.len()])) % 256;
        state.swap(i, j);
    }
    let (mut i, mut j) = (0, 0);
    bytes
        .iter()
        .map(|byte| {
            i = (i + 1) % 256;
            j = (j + state[i]) % 256;
            state.swap(i, j);
            byte ^ state[(state[i] + state[j]) % 256] as u8
        })
        .collect()
}

fn blend(c0: u8, c1: u8, r0: u8, r1: u8) -> [u8; 4] {
    [
        (r0 & 170) | (c0 & 85),
        (r0 & 85) | (c0 & 170),
        (r1 & 170) | (c1 & 85),
        (r1 & 85) | (c1 & 170),
    ]
}

#[derive(Clone, Copy)]
pub enum Target {
    Login,
    Main,
}

pub fn abogus(query: &str, body: &str, counter: u32, target: Target) -> String {
    login_abogus_with(
        query,
        body,
        chrono::Utc::now().timestamp_millis() as u64,
        counter,
        target,
        rand::random::<f64>,
    )
}

fn login_abogus_with(
    query: &str,
    body: &str,
    now: u64,
    counter: u32,
    target: Target,
    mut random: impl FnMut() -> f64,
) -> String {
    const PERM: [usize; 50] = [
        34, 44, 56, 61, 73, 29, 70, 45, 35, 49, 38, 66, 51, 68, 28, 48, 64, 47, 30, 71, 26, 55, 31, 69, 59, 40, 62, 63,
        27, 72, 41, 74, 57, 52, 42, 39, 33, 67, 53, 43, 65, 46, 36, 24, 60, 32, 79, 80, 84, 85,
    ];
    let hash = |text: &str| Sm3::digest(Sm3::digest(format!("{text}dhzx").as_bytes()));
    let h1 = hash(query);
    let h2 = hash(body);
    let (minor, page_id) = match target {
        Target::Login => (14u8, 6241u32),
        Target::Main => (8, 11881),
    };
    let h_ua = Sm3::digest(
        encode(
            &rc4_variant(UA.as_bytes(), &[0, 1, minor]),
            "ckdp1h4ZKsUB80/Mfvw36XIgR25+WQAlEi7NLboqYTOPuzmFjJnryx9HVGDaStCe",
        )
        .as_bytes(),
    );
    let mut fields = [0u8; 86];
    fields[24] = 41;
    fields[26] = ((now - 1_721_836_800_000) / 1000 / 60 / 60 / 24 / 14) as u8;
    fields[27] = if counter > 10745 {
        3
    } else if counter > 1283 {
        4
    } else if counter > 139 {
        5
    } else {
        6
    };
    fields[28] = 3;
    fields[29..35].copy_from_slice(&now.to_le_bytes()[..6]);
    fields[35] = 1;
    fields[38] = 1;
    if matches!(target, Target::Main) {
        fields[40..43].copy_from_slice(&[132, 1, 1]);
    }
    fields[44] = minor;
    fields[48] = h1[9];
    fields[49] = h1[18];
    let escape = |digest: &[u8], start: usize, reserved: u8, fallback: u8| {
        digest[start..]
            .iter()
            .copied()
            .find(|n| *n != reserved)
            .unwrap_or(fallback)
    };
    fields[51] = escape(&h1, 3, 11, 12);
    fields[52] = h2[10];
    fields[53] = h2[19];
    fields[55] = escape(&h2, 4, 8, 9);
    fields[56] = h_ua[11];
    fields[57] = h_ua[21];
    fields[59] = escape(&h_ua, 5, 12, 13);
    fields[60..66].copy_from_slice(&(now - 1).to_le_bytes()[..6]);
    fields[66] = 3;
    fields[67..71].copy_from_slice(&page_id.to_le_bytes());
    fields[71..75].copy_from_slice(&6383u32.to_le_bytes());
    let geometry = b"2560|1215|2560|1392|2560|1392|2560|1440|Win32";
    let timer = format!("{},", (now + 3) & 255);
    fields[79..81].copy_from_slice(&(geometry.len() as u16).to_le_bytes());
    fields[84..86].copy_from_slice(&(timer.len() as u16).to_le_bytes());
    let first = (random() * 65535.0) as u16;
    let mut a8 = blend(1, 0, first as u8, (first >> 8) as u8).to_vec();
    random();
    let check = (random() * 240.0).floor() as u8;
    let check = if check > 109 { check + check % 2 + 1 } else { check };
    let perm = ((random() * 255.0).floor() as u8 & 77) | 2 | 16 | 32 | 128;
    a8.extend(blend(1, 0, check, perm));
    let checksum = a8.iter().chain(PERM.iter().map(|i| &fields[*i])).fold(0, |a, b| a ^ b);
    let mut a98: Vec<u8> = PERM.iter().map(|i| fields[*i]).collect();
    a98.extend(geometry);
    a98.extend(timer.as_bytes());
    a98.push(checksum);
    let hr0 = (random() * 65535.0).floor() as u32 as u8;
    let offset = (random() * 40.0).floor() as u8;
    for group in a98.chunks(3) {
        if group.len() == 3 {
            let r = (random() * 1000.0).floor() as u32 as u8;
            a8.extend([
                (r & 145) | (group[0] & 110),
                (r & 66) | (group[1] & 189),
                (r & 44) | (group[2] & 211),
                (group[0] & 145) | (group[1] & 66) | (group[2] & 44),
            ]);
        } else {
            a8.push(group[0]);
            if group.len() == 2 && group[1] != 0 {
                a8.push(group[1]);
            }
        }
    }
    let mut bytes = blend(3, 82, hr0, offset).to_vec();
    bytes.extend(rc4_variant(&a8, &[211]));
    encode(
        &bytes,
        "Dkdpgh2ZmsQB80/MfvV36XI1R45-WUAlEixNLwoqYTOPuzKFjJnry79HbGcaStCe",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn modern_login_and_token_vectors() {
        let vectors: serde_json::Value = serde_json::from_str(include_str!("passport-modern-vectors.json")).unwrap();
        for v in vectors.as_array().unwrap() {
            let pairs: Vec<_> = v["pairs"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| (p[0].as_str().unwrap(), p[1].as_str().unwrap().to_owned()))
                .collect();
            let encoded = query(&pairs);
            assert_eq!(encoded, v["query"]);
            assert_eq!(
                web_sign(&encoded, "synthetic-uifid", v["now"].as_i64().unwrap() / 1000),
                v["web_signed"]
            );
            let mut index = 0;
            let randoms = [0.0, 0.125, 0.99999, 0.75, 0.33, 0.5];
            assert_eq!(
                login_abogus_with(
                    &encoded,
                    v["body"].as_str().unwrap(),
                    v["now"].as_u64().unwrap(),
                    v["counter"].as_u64().unwrap() as u32,
                    if v["host"] == "www.douyin.com" {
                        Target::Main
                    } else {
                        Target::Login
                    },
                    || {
                        let r = randoms[index % randoms.len()];
                        index += 1;
                        r
                    }
                ),
                v["abogus"]
            );
            assert_eq!(
                str_data(v["plaintext"].as_str().unwrap(), v["nonce"].as_u64().unwrap() as u8),
                v["str_data"]
            );
        }
    }
    #[test]
    fn matches_pinned_typescript_reference_vectors() {
        let vectors: serde_json::Value = serde_json::from_str(include_str!("passport-sign-vectors.json")).unwrap();
        for v in vectors.as_array().unwrap() {
            let pairs = |key: &str| {
                v[key]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|p| (p[0].as_str().unwrap(), p[1].as_str().unwrap().to_owned()))
                    .collect::<Vec<_>>()
            };
            let (sign, qs) = seal(&pairs("query"), &pairs("body"));
            assert_eq!(sign, v["sign"]);
            assert_eq!(qs, v["qs"]);
            assert_eq!(sdk_ts(v["now_ms"].as_i64().unwrap()).unwrap(), v["ts"]);
            assert_eq!(
                aid_sign(v["path"].as_str().unwrap(), v["ts"].as_str().unwrap()),
                v["aid_sign"]
            );
            assert_eq!(encrypt(v["plaintext"].as_str().unwrap()), v["encrypted"]);
        }
    }
}
