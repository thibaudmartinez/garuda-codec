/// Takes two vectors as input and returns an iterator that yields elements alternating between
/// the longer and the shorter vectors. The elements from the shorter vector are distributed
/// approximately evenly across the longer vector.
/// Items are yielded in reverse order starting from the end of each vector.
pub fn interleave<T>(v1: Vec<T>, v2: Vec<T>) -> impl Iterator<Item = T> {
    let (mut long, mut short) = if v1.len() >= v2.len() {
        (v1, v2)
    } else {
        (v2, v1)
    };

    let long_len = long.len();
    let short_len = short.len();
    let total_len = long_len + short_len;

    let mut short_idx: usize = 0;

    // Iterate over input vectors in reverse order, so items are popped from the end of the vectors.
    // Popping has O(1) complexity, while removing the head of the vector would be O(n).
    (0..total_len).rev().filter_map(move |idx| {
        if short_idx < short_len {
            let target = total_len - (short_idx * total_len) / short_len - 1;

            // If current position matches the target, return from the shorter vector.
            if idx == target {
                short_idx += 1;
                return short.pop();
            }
        }

        // Otherwise, returns from the longer vector.
        long.pop()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_equal_length() {
        let v1 = vec![1, 3, 5];
        let v2 = vec![2, 4, 6];
        let result: Vec<_> = interleave(v1, v2).collect();

        assert_eq!(result, vec![6, 5, 4, 3, 2, 1]);
    }

    #[test]
    fn test_different_length() {
        let v1 = vec![10, 20, 30, 40, 50];
        let v2 = vec![1, 2];
        let result: Vec<_> = interleave(v1, v2).collect();

        assert_eq!(result, vec![2, 50, 40, 1, 30, 20, 10]);
    }

    #[test]
    fn test_one_empty() {
        let v1: Vec<i32> = vec![];
        let v2 = vec![1, 2, 3];
        let result: Vec<_> = interleave(v1, v2).collect();

        assert_eq!(result, vec![3, 2, 1]);
    }

    #[test]
    fn test_both_empty() {
        let v1: Vec<i32> = vec![];
        let v2: Vec<i32> = vec![];
        let result: Vec<_> = interleave(v1, v2).collect();

        assert!(result.is_empty());
    }
}
