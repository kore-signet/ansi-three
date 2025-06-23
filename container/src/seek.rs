use std::io::{self, Read};

use integer_encoding::{VarIntReader, VarIntWriter};

/*

Seek table format:
    stream_index : u8
    len_bytes : u64
    len_elements : u64
    [encoded bytes for time + location : LZ4]
*/

#[derive(PartialEq, PartialOrd, Copy, Clone, Debug)]
pub struct SeekEntry {
    pub ts: i64,
    pub location: i64,
}

pub fn delta_encode(mut iter: impl Iterator<Item = i64>) -> Vec<u8> {
    let mut out = Vec::new();
    let initial = iter.next().unwrap();
    out.write_varint(initial).unwrap();

    let mut prev_val = initial;
    let mut prev_delta = 0;

    for val in iter {
        let delta = val - prev_val;
        let delta_of_delta = delta - prev_delta;

        out.write_varint(delta_of_delta).unwrap();

        prev_delta = delta;
        prev_val = val;
    }

    out
}

pub fn delta_decode(input: &mut impl Read, len: usize) -> io::Result<Vec<i64>> {
    let mut prev_val: i64 = input.read_varint()?;
    let mut prev_delta = 0;

    let mut out = Vec::new();
    out.push(prev_val);

    for _ in 0..(len - 1) {
        let delta_of_delta: i64 = input.read_varint()?;
        prev_delta += delta_of_delta;

        prev_val += prev_delta;

        out.push(prev_val);
    }

    Ok(out)
}

#[cfg(test)]
mod test {
    use tinyrand::{RandRange, StdRand};

    use crate::seek::{delta_decode, delta_encode};

    #[test]
    fn test_delta() {
        let mut input = Vec::from_iter((0i64..3001i64).map(|v| v * 1312));
        let len = input.len();
        let mut encoded = delta_encode(input.clone().into_iter());
        let decoded = delta_decode(&mut encoded.as_slice(), len).unwrap();

        assert_eq!(decoded, input);
    }

    #[test]
    fn test_delta_rand() {
        let mut rng = StdRand::default();

        let len: usize = rng.next_range(5000..10000);

        let mut val = 0;
        let mut input = Vec::with_capacity(len);
        for _ in 0..len {
            val += rng.next_range(5000u64..20000u64) as i64;
            input.push(val);
        }

        let mut encoded = delta_encode(input.clone().into_iter());
        let decoded = delta_decode(&mut encoded.as_slice(), len).unwrap();

        assert_eq!(decoded, input);
    }
}
