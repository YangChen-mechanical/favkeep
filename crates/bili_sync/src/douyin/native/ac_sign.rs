// SPDX-License-Identifier: MIT
// Derived from official SDK 76bee010ec645500ff8012396694a708c5b38e812d8822544e312485e0002a5b.
const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-.";

fn hash(mut seed: u32, text: &str) -> u32 {
    for unit in text.encode_utf16() {
        seed = (seed ^ u32::from(unit)).wrapping_mul(65_599);
    }
    seed
}

fn encode30(value: u32) -> String {
    [24, 18, 12, 6, 0]
        .into_iter()
        .map(|shift| ALPHABET[((value >> shift) & 63) as usize] as char)
        .collect()
}

pub fn signature(nonce: &str, href: &str, ua: &str, seconds: u64) -> String {
    let seconds = seconds as u32;
    let href = href.split_once("://").map_or(href, |(_, path)| path);
    let url_hash = hash(hash(0, &seconds.to_string()), href) % 65_521;
    let number = (0x2030_u64 << 32) | u64::from(seconds ^ (url_hash * 65_521));
    let number_hash = hash(0, &number.to_string());
    let ua_nonce = ((hash(number_hash, ua) % 65_521) << 16) | (hash(number_hash, nonce) % 65_521);
    // shortcut: one fixed public device profile, revalidate it against the official SDK when its schema changes.
    let words = [
        number as u32,
        (number >> 32) as u32,
        1_666_195_044 ^ number as u32,
        ua_nonce,
        ((2_049 << 8) | (2 << 4)) ^ number as u32,
    ];
    let mut result = String::from("_02B4Z6wo00f01");
    let mut previous = 0;
    for (index, word) in words.into_iter().enumerate() {
        match index % 3 {
            0 => result.push_str(&encode30(word >> 2)),
            1 => result.push_str(&encode30((previous << 28) | (word >> 4))),
            _ => {
                result.push_str(&encode30((previous << 26) | (word >> 6)));
                result.push(ALPHABET[(word & 63) as usize] as char);
            }
        }
        previous = word;
    }
    result.push_str(&encode30(url_hash));
    let checksum = result.bytes().fold(0_u32, |seed, byte| {
        seed.wrapping_mul(65_599).wrapping_add(u32::from(byte))
    });
    let checksum = format!("{checksum:x}");
    result.push_str(&checksum[checksum.len().saturating_sub(2)..]);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matches_official_sdk_for_fixed_synthetic_inputs_and_timestamp_wrap() {
        let fixtures: serde_json::Value = serde_json::from_str(include_str!("ac-sign-vectors.json")).unwrap();
        for vector in fixtures["vectors"].as_array().unwrap() {
            assert_eq!(
                signature(
                    vector["nonce"].as_str().unwrap(),
                    vector["href"].as_str().unwrap(),
                    vector["ua"].as_str().unwrap(),
                    vector["seconds"].as_u64().unwrap()
                ),
                vector["signature"].as_str().unwrap()
            );
        }
    }
}
