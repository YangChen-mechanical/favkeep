// SPDX-License-Identifier: Apache-2.0
// Port of F2's abogus.py, copyright JohnserfSeed, licensed under Apache-2.0.
// https://github.com/Johnserf-Seed/f2/blob/f6be8c0ffba9a127075bbeafe4838716650b6325/f2/utils/crypto/bytedance/abogus.py
use sm3::{Digest, Sm3};

const ALPHABET: &[u8; 64] = b"Dkdpgh2ZmsQB80/MfvV36XI1R45-WUAlEixNLwoqYTOPuzKFjJnry79HbGcaStCe";
const UA_ALPHABET: &[u8; 64] = b"ckdp1h4ZKsUB80/Mfvw36XIgR25+WQAlEi7NLboqYTOPuzmFjJnryx9HVGDaStCe";
const DEFAULT_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36 Edg/130.0.0.0";
const SORT_INDEX: [usize; 44] = [
    18, 20, 52, 26, 30, 34, 58, 38, 40, 53, 42, 21, 27, 54, 55, 31, 35, 57, 39, 41, 43, 22, 28, 32, 60, 36, 23, 29, 33,
    37, 44, 45, 59, 46, 47, 48, 49, 50, 24, 25, 65, 66, 70, 71,
];
const PERMUTATION: [u8; 256] = [
    121, 243, 55, 234, 103, 36, 47, 228, 30, 231, 106, 6, 115, 95, 78, 101, 250, 207, 198, 50, 139, 227, 220, 105, 97,
    143, 34, 28, 194, 215, 18, 100, 159, 160, 43, 8, 169, 217, 180, 120, 247, 45, 90, 11, 27, 197, 46, 3, 84, 72, 5,
    68, 62, 56, 221, 75, 144, 79, 73, 161, 178, 81, 64, 187, 134, 117, 186, 118, 16, 241, 130, 71, 89, 147, 122, 129,
    65, 40, 88, 150, 110, 219, 199, 255, 181, 254, 48, 4, 195, 248, 208, 32, 116, 167, 69, 201, 17, 124, 125, 104, 96,
    83, 80, 127, 236, 108, 154, 126, 204, 15, 20, 135, 112, 158, 13, 1, 188, 164, 210, 237, 222, 98, 212, 77, 253, 42,
    170, 202, 26, 22, 29, 182, 251, 10, 173, 152, 58, 138, 54, 141, 185, 33, 157, 31, 252, 132, 233, 235, 102, 196,
    191, 223, 240, 148, 39, 123, 92, 82, 128, 109, 57, 24, 38, 113, 209, 245, 2, 119, 153, 229, 189, 214, 230, 174,
    232, 63, 52, 205, 86, 140, 66, 175, 111, 171, 246, 133, 238, 193, 99, 60, 74, 91, 225, 51, 76, 37, 145, 211, 166,
    151, 213, 206, 0, 200, 244, 176, 218, 44, 184, 172, 49, 216, 93, 168, 53, 21, 183, 41, 67, 85, 224, 155, 226, 242,
    87, 177, 146, 70, 190, 12, 162, 19, 137, 114, 25, 165, 163, 192, 23, 59, 9, 94, 179, 107, 35, 7, 142, 131, 239,
    203, 149, 136, 61, 249, 14, 156,
];

/// Sign the query and body exactly as transmitted. UA must contain only Latin-1
/// characters (normally ASCII); supply the browser's explicit fingerprint.
pub fn sign(query: &str, body: &str, ua: &str, fingerprint: &str) -> String {
    sign_with_clock(
        query,
        body,
        ua,
        fingerprint,
        || chrono::Utc::now().timestamp_millis() as u64,
        std::array::from_fn(|_| rand::random_range(0..10000)),
    )
}

/// Deterministic inputs match F2's two millisecond timestamps and its three
/// `int(random.random() * 10000)` values, each in 0..10000.
#[cfg(test)]
pub fn sign_with_inputs(
    query: &str,
    body: &str,
    ua: &str,
    fingerprint: &str,
    start_ms: u64,
    end_ms: u64,
    random: [u16; 3],
) -> String {
    let mut times = [start_ms, end_ms].into_iter();
    sign_with_clock(query, body, ua, fingerprint, || times.next().unwrap(), random)
}

fn sign_with_clock(
    query: &str,
    body: &str,
    ua: &str,
    fingerprint: &str,
    mut clock: impl FnMut() -> u64,
    random: [u16; 3],
) -> String {
    let start = clock();
    let query_hash = salted_hash(query);
    let body_hash = salted_hash(body);
    let ua = if ua.is_empty() { DEFAULT_UA } else { ua };
    let encrypted_ua = rc4_ua(ua);
    let ua_hash = Sm3::digest(encode(&encrypted_ua, UA_ALPHABET).as_bytes());
    let end = clock();
    let mut fields = [0u32; 72];
    fields[18] = 44;
    for (offset, shift) in [24, 16, 8, 0].into_iter().enumerate() {
        fields[20 + offset] = ((start >> shift) & 255) as u32;
        fields[44 + offset] = ((end >> shift) & 255) as u32;
    }
    // F2 deliberately keeps the high timestamp values wider than a byte.
    fields[24] = (start >> 32) as u32;
    fields[25] = (start >> 40) as u32;
    fields[49] = (end >> 32) as u32;
    fields[50] = (end >> 40) as u32;
    fields[31] = 1;
    fields[37] = 14;
    fields[38] = query_hash[21] as u32;
    fields[39] = query_hash[22] as u32;
    fields[40] = body_hash[21] as u32;
    fields[41] = body_hash[22] as u32;
    fields[42] = ua_hash[23] as u32;
    fields[43] = ua_hash[24] as u32;
    fields[48] = 3;
    fields[57] = 6383 & 255;
    fields[58] = 6383 >> 8;
    fields[65] = fingerprint.chars().count() as u32;
    let mut values: Vec<u32> = SORT_INDEX.iter().map(|&i| fields[i]).collect();
    let checksum = values.iter().fold(0, |value, next| value ^ next);
    values.extend(fingerprint.chars().map(u32::from));
    values.push(checksum);

    let mut result = Vec::with_capacity(12 + values.len());
    for value in random {
        assert!(value < 10000, "A-Bogus random input must be below 10000");
        let value = value as u32;
        result.extend([
            (value & 170) | 1,
            (value & 85) | 2,
            ((value >> 8) & 170) | 5,
            ((value >> 8) & 85) | 40,
        ]);
    }
    result.extend(transform(&values));
    encode(&result, ALPHABET)
}

fn salted_hash(value: &str) -> [u8; 32] {
    let mut hash = Sm3::new();
    hash.update(value.as_bytes());
    hash.update(b"cus");
    Sm3::digest(hash.finalize()).into()
}

fn rc4_ua(ua: &str) -> Vec<u32> {
    let key = [0usize, 1, 14];
    let mut state: [u8; 256] = std::array::from_fn(|i| i as u8);
    let mut j = 0;
    for i in 0..256 {
        j = (j + state[i] as usize + key[i % 3]) % 256;
        state.swap(i, j);
    }
    let mut i = 0;
    j = 0;
    ua.chars()
        .map(|character| {
            let value = u8::try_from(u32::from(character)).expect("A-Bogus UA must contain only Latin-1 characters");
            i = (i + 1) % 256;
            j = (j + state[i] as usize) % 256;
            state.swap(i, j);
            (value ^ state[(state[i] as usize + state[j] as usize) % 256]) as u32
        })
        .collect()
}

fn transform(values: &[u32]) -> Vec<u32> {
    let mut state = PERMUTATION;
    let mut index_b = state[1] as usize;
    let mut initial = 0;
    let mut value_e = 0;
    values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let sum = if index == 0 {
                initial = state[index_b] as usize;
                state[1] = initial as u8;
                state[index_b] = index_b as u8;
                index_b + initial
            } else {
                initial + value_e
            };
            let encrypted = value ^ state[sum % 256] as u32;
            let next = (index + 2) % 256;
            value_e = state[next] as usize;
            index_b = (index_b + value_e) % 256;
            initial = state[index_b] as usize;
            state.swap(index_b, next);
            encrypted
        })
        .collect()
}

fn encode(values: &[u32], alphabet: &[u8; 64]) -> String {
    let mut encoded = String::with_capacity(values.len().div_ceil(3) * 4);
    for chunk in values.chunks(3) {
        let value = (chunk[0] << 16) | (chunk.get(1).copied().unwrap_or(0) << 8) | chunk.get(2).copied().unwrap_or(0);
        encoded.push(alphabet[((value >> 18) & 63) as usize] as char);
        encoded.push(alphabet[((value >> 12) & 63) as usize] as char);
        encoded.push(if chunk.len() > 1 {
            alphabet[((value >> 6) & 63) as usize] as char
        } else {
            '='
        });
        encoded.push(if chunk.len() > 2 {
            alphabet[(value & 63) as usize] as char
        } else {
            '='
        });
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_fixed_python_vectors() {
        let fixtures: serde_json::Value = serde_json::from_str(include_str!("fixtures/sign.json")).unwrap();
        for case in fixtures["vectors"].as_array().unwrap() {
            let random = std::array::from_fn(|i| case["random"][i].as_u64().unwrap() as u16);
            let actual = sign_with_inputs(
                case["query"].as_str().unwrap(),
                case["body"].as_str().unwrap(),
                case["ua"].as_str().unwrap(),
                case["fingerprint"].as_str().unwrap(),
                case["start_ms"].as_u64().unwrap(),
                case["end_ms"].as_u64().unwrap(),
                random,
            );
            assert_eq!(actual, case["signature"].as_str().unwrap(), "{}", case["name"]);
        }
    }
}
