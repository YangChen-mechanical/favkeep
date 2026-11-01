// SPDX-License-Identifier: MIT
// Standard RC4 used by platform request encodings.
pub fn rc4(bytes: &[u8], key: &[u8]) -> Vec<u8> {
    assert!(!key.is_empty());
    let mut state = std::array::from_fn::<_, 256, _>(|i| i as u8);
    let mut j = 0usize;
    for i in 0..256 {
        j = (j + usize::from(state[i]) + usize::from(key[i % key.len()])) % 256;
        state.swap(i, j);
    }
    let (mut i, mut j) = (0, 0);
    bytes
        .iter()
        .map(|byte| {
            i = (i + 1) % 256;
            j = (j + usize::from(state[i])) % 256;
            state.swap(i, j);
            byte ^ state[(usize::from(state[i]) + usize::from(state[j])) % 256]
        })
        .collect()
}
